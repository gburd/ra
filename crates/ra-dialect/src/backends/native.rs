//! Native SQL dialect translation backend.
//!
//! This module contains the original hand-written translation
//! logic for the core 6 dialects.

use sqlparser::ast::{
    self, Expr, FunctionArg, FunctionArgExpr,
    FunctionArguments, Ident, ObjectName, Query, SelectItem,
    SetExpr, Statement, Value,
};
use sqlparser::parser::Parser;

use crate::dialect::Dialect;
use crate::error::{
    TranslationError, TranslationWarning, WarningSeverity,
};
use crate::functions::build_function_map;
use crate::{Backend, TranslationResult};

type ExprResult =
    Result<(Expr, Vec<TranslationWarning>), TranslationError>;

/// Native translation backend implementation.
pub struct NativeBackend;

impl Backend for NativeBackend {
    fn translate(
        &self,
        sql: &str,
        source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError> {
        let source_parser = source.sqlparser_dialect();
        let statements =
            Parser::parse_sql(&*source_parser, sql)?;

        let mut warnings = Vec::new();
        let mut translated_statements = Vec::new();

        for statement in statements {
            let (translated, mut stmt_warnings) =
                translate_statement(
                    statement, source, target,
                )?;
            warnings.append(&mut stmt_warnings);
            translated_statements.push(translated);
        }

        let sql = translated_statements.join(";\n");

        Ok(TranslationResult { sql, warnings })
    }
}

/// Translate a single statement.
fn translate_statement(
    statement: Statement,
    source: Dialect,
    target: Dialect,
) -> Result<(String, Vec<TranslationWarning>), TranslationError>
{
    let mut warnings = Vec::new();

    let translated = match statement {
        Statement::Query(query) => {
            let (translated_query, mut query_warnings) =
                translate_query(*query, source, target)?;
            warnings.append(&mut query_warnings);
            Statement::Query(Box::new(translated_query))
        }
        _ => statement,
    };

    Ok((translated.to_string(), warnings))
}

/// Translate a query, including CTEs, body, ORDER BY,
/// and LIMIT/OFFSET.
fn translate_query(
    mut query: Query,
    source: Dialect,
    target: Dialect,
) -> Result<(Query, Vec<TranslationWarning>), TranslationError>
{
    let mut warnings = Vec::new();

    // Translate CTE bodies
    if let Some(ref mut with) = query.with {
        for cte in &mut with.cte_tables {
            let (translated, mut w) = translate_query(
                *cte.query.clone(),
                source,
                target,
            )?;
            warnings.append(&mut w);
            *cte.query = translated;
        }
    }

    // Translate the query body (SELECT, UNION, etc.)
    let (body, mut body_warnings) =
        translate_set_expr(*query.body, source, target)?;
    warnings.append(&mut body_warnings);
    query.body = Box::new(body);

    // Translate ORDER BY expressions
    if let Some(ref mut order_by) = query.order_by {
        for item in &mut order_by.exprs {
            let (translated, mut w) = translate_expr(
                item.expr.clone(),
                source,
                target,
            )?;
            warnings.append(&mut w);
            item.expr = translated;
        }
    }

    // Handle LIMIT -> FETCH for MSSQL/Oracle
    if source != target {
        if let Some(limit_expr) = query.limit.take() {
            if !target.supports_limit()
                && target.supports_fetch()
            {
                query.fetch = Some(ast::Fetch {
                    with_ties: false,
                    percent: false,
                    quantity: Some(limit_expr),
                });
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!(
                        "LIMIT translated to FETCH for \
                         {target}"
                    ),
                    hint: Some(
                        "FETCH requires ORDER BY in MSSQL"
                            .into(),
                    ),
                });
            } else {
                query.limit = Some(limit_expr);
            }
        }
    }

    Ok((query, warnings))
}

/// Translate a set expression (SELECT, UNION, etc.).
fn translate_set_expr(
    set_expr: SetExpr,
    source: Dialect,
    target: Dialect,
) -> Result<(SetExpr, Vec<TranslationWarning>), TranslationError>
{
    let mut warnings = Vec::new();

    let translated = match set_expr {
        SetExpr::Select(select) => {
            let (s, mut w) =
                translate_select(*select, source, target)?;
            warnings.append(&mut w);
            s
        }
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => {
            let (left, mut lw) =
                translate_set_expr(*left, source, target)?;
            warnings.append(&mut lw);
            let (right, mut rw) =
                translate_set_expr(*right, source, target)?;
            warnings.append(&mut rw);
            SetExpr::SetOperation {
                op,
                set_quantifier,
                left: Box::new(left),
                right: Box::new(right),
            }
        }
        other => other,
    };

    Ok((translated, warnings))
}

/// Translate a SELECT clause: projection, WHERE, HAVING,
/// GROUP BY.
fn translate_select(
    mut select: ast::Select,
    source: Dialect,
    target: Dialect,
) -> Result<(SetExpr, Vec<TranslationWarning>), TranslationError>
{
    let mut warnings = Vec::new();

    for item in &mut select.projection {
        match item {
            SelectItem::UnnamedExpr(expr)
            | SelectItem::ExprWithAlias { expr, .. } => {
                let (translated, mut w) =
                    translate_expr(
                        expr.clone(),
                        source,
                        target,
                    )?;
                warnings.append(&mut w);
                *expr = translated;
            }
            _ => {}
        }
    }

    if let Some(ref mut where_clause) = select.selection {
        let (translated, mut w) = translate_expr(
            where_clause.clone(),
            source,
            target,
        )?;
        warnings.append(&mut w);
        *where_clause = translated;
    }

    if let Some(ref mut having) = select.having {
        let (translated, mut w) = translate_expr(
            having.clone(),
            source,
            target,
        )?;
        warnings.append(&mut w);
        *having = translated;
    }

    if let ast::GroupByExpr::Expressions(
        ref mut exprs,
        _,
    ) = select.group_by
    {
        for expr in exprs.iter_mut() {
            let (translated, mut w) = translate_expr(
                expr.clone(),
                source,
                target,
            )?;
            warnings.append(&mut w);
            *expr = translated;
        }
    }

    Ok((SetExpr::Select(Box::new(select)), warnings))
}

/// Translate an expression, dispatching to specialized
/// helpers for each expression variant.
fn translate_expr(
    expr: Expr,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    match expr {
        Expr::BinaryOp {
            left,
            op: ast::BinaryOperator::StringConcat,
            right,
        } => translate_string_concat(
            *left, *right, source, target,
        ),
        Expr::BinaryOp { left, op, right } => {
            translate_binary_op(
                *left, op, *right, source, target,
            )
        }
        Expr::ILike {
            negated,
            expr: inner,
            pattern,
            escape_char,
            ..
        } => translate_ilike(
            negated, *inner, *pattern, escape_char,
            source, target,
        ),
        Expr::Value(Value::Boolean(b))
            if !target.supports_boolean_literals() =>
        {
            Ok(translate_boolean(b, target))
        }
        Expr::Cast {
            expr: inner,
            data_type,
            format,
            kind,
        } => translate_cast(
            *inner, data_type, format, kind, source,
            target,
        ),
        Expr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => translate_case(
            operand, conditions, results, else_result,
            source, target,
        ),
        Expr::Function(func) => {
            translate_function(func, source, target)
        }
        Expr::Between {
            expr: inner,
            negated,
            low,
            high,
        } => translate_between(
            *inner, negated, *low, *high, source, target,
        ),
        Expr::InSubquery {
            expr: inner,
            subquery,
            negated,
        } => translate_in_subquery(
            *inner, *subquery, negated, source, target,
        ),
        other => translate_expr_simple(
            other, source, target,
        ),
    }
}

/// Handle expression variants that only need simple
/// recursion into a single child.
fn translate_expr_simple(
    expr: Expr,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    match expr {
        Expr::Nested(inner) => {
            let (te, w) =
                translate_expr(*inner, source, target)?;
            Ok((Expr::Nested(Box::new(te)), w))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let (te, w) =
                translate_expr(*inner, source, target)?;
            Ok((Expr::UnaryOp { op, expr: Box::new(te) }, w))
        }
        Expr::IsNull(inner) => {
            let (te, w) =
                translate_expr(*inner, source, target)?;
            Ok((Expr::IsNull(Box::new(te)), w))
        }
        Expr::IsNotNull(inner) => {
            let (te, w) =
                translate_expr(*inner, source, target)?;
            Ok((Expr::IsNotNull(Box::new(te)), w))
        }
        Expr::Subquery(subquery) => {
            let (tq, w) =
                translate_query(*subquery, source, target)?;
            Ok((Expr::Subquery(Box::new(tq)), w))
        }
        Expr::Exists { subquery, negated } => {
            let (tq, w) =
                translate_query(*subquery, source, target)?;
            Ok((
                Expr::Exists {
                    subquery: Box::new(tq),
                    negated,
                },
                w,
            ))
        }
        other => Ok((other, Vec::new())),
    }
}

/// Translate `||` string concatenation to `+` for MSSQL or
/// `CONCAT` for `MySQL`.
fn translate_string_concat(
    left: Expr,
    right: Expr,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (tl, mut lw) =
        translate_expr(left, source, target)?;
    warnings.append(&mut lw);
    let (tr, mut rw) =
        translate_expr(right, source, target)?;
    warnings.append(&mut rw);

    let result = match target {
        Dialect::MsSql => {
            warnings.push(TranslationWarning {
                severity: WarningSeverity::Info,
                message:
                    "|| translated to + for MSSQL \
                     string concatenation"
                        .into(),
                hint: None,
            });
            Expr::BinaryOp {
                left: Box::new(tl),
                op: ast::BinaryOperator::Plus,
                right: Box::new(tr),
            }
        }
        Dialect::MySql => {
            warnings.push(TranslationWarning {
                severity: WarningSeverity::Info,
                message:
                    "|| translated to CONCAT() for MySQL"
                        .into(),
                hint: None,
            });
            make_concat_call(tl, tr)
        }
        _ => Expr::BinaryOp {
            left: Box::new(tl),
            op: ast::BinaryOperator::StringConcat,
            right: Box::new(tr),
        },
    };
    Ok((result, warnings))
}

/// Translate a generic binary operator by recursing into
/// both sides.
fn translate_binary_op(
    left: Expr,
    op: ast::BinaryOperator,
    right: Expr,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (tl, mut lw) =
        translate_expr(left, source, target)?;
    warnings.append(&mut lw);
    let (tr, mut rw) =
        translate_expr(right, source, target)?;
    warnings.append(&mut rw);
    Ok((
        Expr::BinaryOp {
            left: Box::new(tl),
            op,
            right: Box::new(tr),
        },
        warnings,
    ))
}

/// Translate ILIKE to `LOWER()` + LIKE for dialects that
/// lack native ILIKE support.
fn translate_ilike(
    negated: bool,
    inner: Expr,
    pattern: Expr,
    escape_char: Option<String>,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (te, mut ew) =
        translate_expr(inner, source, target)?;
    warnings.append(&mut ew);
    let (tp, mut pw) =
        translate_expr(pattern, source, target)?;
    warnings.append(&mut pw);

    if target.supports_ilike() {
        return Ok((
            Expr::ILike {
                negated,
                any: false,
                expr: Box::new(te),
                pattern: Box::new(tp),
                escape_char,
            },
            warnings,
        ));
    }

    warnings.push(TranslationWarning {
        severity: WarningSeverity::Info,
        message: format!(
            "ILIKE translated to LOWER() + LIKE for {target}"
        ),
        hint: Some(
            "LOWER() may not handle all Unicode case \
             folding correctly"
                .into(),
        ),
    });
    Ok((
        Expr::Like {
            negated,
            any: false,
            expr: Box::new(wrap_in_lower(te)),
            pattern: Box::new(wrap_in_lower(tp)),
            escape_char,
        },
        warnings,
    ))
}

/// Translate TRUE/FALSE to 1/0 for dialects that lack
/// boolean literals.
fn translate_boolean(
    b: bool,
    target: Dialect,
) -> (Expr, Vec<TranslationWarning>) {
    let int_val = i32::from(b);
    let warning = TranslationWarning {
        severity: WarningSeverity::Info,
        message: format!(
            "Boolean literal translated to {int_val} \
             for {target}"
        ),
        hint: None,
    };
    (
        Expr::Value(Value::Number(
            int_val.to_string(),
            false,
        )),
        vec![warning],
    )
}

/// Translate CAST expressions, converting `::` shorthand
/// to standard CAST for dialects that need it.
fn translate_cast(
    inner: Expr,
    data_type: ast::DataType,
    format: Option<ast::CastFormat>,
    kind: ast::CastKind,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (te, mut ew) =
        translate_expr(inner, source, target)?;
    warnings.append(&mut ew);

    let new_kind = if kind == ast::CastKind::DoubleColon
        && !target.supports_double_colon_cast()
    {
        warnings.push(TranslationWarning {
            severity: WarningSeverity::Info,
            message: format!(
                ":: cast translated to CAST() for {target}"
            ),
            hint: None,
        });
        ast::CastKind::Cast
    } else {
        kind
    };

    Ok((
        Expr::Cast {
            expr: Box::new(te),
            data_type,
            format,
            kind: new_kind,
        },
        warnings,
    ))
}

/// Translate CASE expressions by recursing into operand,
/// conditions, results, and else branch.
fn translate_case(
    operand: Option<Box<Expr>>,
    conditions: Vec<Expr>,
    results: Vec<Expr>,
    else_result: Option<Box<Expr>>,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();

    let operand = if let Some(e) = operand {
        let (te, mut w) =
            translate_expr(*e, source, target)?;
        warnings.append(&mut w);
        Some(Box::new(te))
    } else {
        None
    };

    let mut new_conditions = Vec::new();
    for c in conditions {
        let (tc, mut w) =
            translate_expr(c, source, target)?;
        warnings.append(&mut w);
        new_conditions.push(tc);
    }

    let mut new_results = Vec::new();
    for r in results {
        let (tr, mut w) =
            translate_expr(r, source, target)?;
        warnings.append(&mut w);
        new_results.push(tr);
    }

    let else_result = if let Some(e) = else_result {
        let (te, mut w) =
            translate_expr(*e, source, target)?;
        warnings.append(&mut w);
        Some(Box::new(te))
    } else {
        None
    };

    Ok((
        Expr::Case {
            operand,
            conditions: new_conditions,
            results: new_results,
            else_result,
        },
        warnings,
    ))
}

/// Translate a function call: rename, recurse into
/// arguments, and translate window OVER clauses.
fn translate_function(
    mut func: ast::Function,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let function_map = build_function_map(target);
    let func_name = func.name.to_string().to_uppercase();

    if let Some(mapping) = function_map.get(&func_name) {
        let target_name = &mapping.target_name;
        if target_name != &func_name {
            warnings.push(TranslationWarning {
                severity: WarningSeverity::Info,
                message: format!(
                    "Function {func_name} translated \
                     to {target_name}"
                ),
                hint: None,
            });
            func.name =
                ObjectName(vec![Ident::new(target_name)]);
        }
    }

    if let FunctionArguments::List(ref mut args) = func.args
    {
        for arg in &mut args.args {
            if let FunctionArg::Unnamed(
                FunctionArgExpr::Expr(ref mut arg_expr),
            ) = arg
            {
                let (translated_arg, mut aw) =
                    translate_expr(
                        arg_expr.clone(),
                        source,
                        target,
                    )?;
                warnings.append(&mut aw);
                *arg_expr = translated_arg;
            }
        }
    }

    if let Some(ref mut over) = func.over {
        let (translated_over, mut ow) =
            translate_window_type(
                over.clone(),
                source,
                target,
            )?;
        warnings.append(&mut ow);
        *over = translated_over;
    }

    Ok((Expr::Function(func), warnings))
}

/// Translate BETWEEN by recursing into all sub-expressions.
fn translate_between(
    inner: Expr,
    negated: bool,
    low: Expr,
    high: Expr,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (te, mut ew) =
        translate_expr(inner, source, target)?;
    warnings.append(&mut ew);
    let (tl, mut lw) =
        translate_expr(low, source, target)?;
    warnings.append(&mut lw);
    let (th, mut hw) =
        translate_expr(high, source, target)?;
    warnings.append(&mut hw);
    Ok((
        Expr::Between {
            expr: Box::new(te),
            negated,
            low: Box::new(tl),
            high: Box::new(th),
        },
        warnings,
    ))
}

/// Translate IN (subquery) by recursing into the
/// expression and the subquery.
fn translate_in_subquery(
    inner: Expr,
    subquery: Query,
    negated: bool,
    source: Dialect,
    target: Dialect,
) -> ExprResult {
    let mut warnings = Vec::new();
    let (te, mut ew) =
        translate_expr(inner, source, target)?;
    warnings.append(&mut ew);
    let (tq, mut qw) =
        translate_query(subquery, source, target)?;
    warnings.append(&mut qw);
    Ok((
        Expr::InSubquery {
            expr: Box::new(te),
            subquery: Box::new(tq),
            negated,
        },
        warnings,
    ))
}

/// Translate window function OVER clauses.
fn translate_window_type(
    window: ast::WindowType,
    source: Dialect,
    target: Dialect,
) -> Result<
    (ast::WindowType, Vec<TranslationWarning>),
    TranslationError,
> {
    let mut warnings = Vec::new();

    let translated = match window {
        ast::WindowType::WindowSpec(mut spec) => {
            for expr in &mut spec.partition_by {
                let (te, mut w) = translate_expr(
                    expr.clone(),
                    source,
                    target,
                )?;
                warnings.append(&mut w);
                *expr = te;
            }
            for item in &mut spec.order_by {
                let (te, mut w) = translate_expr(
                    item.expr.clone(),
                    source,
                    target,
                )?;
                warnings.append(&mut w);
                item.expr = te;
            }
            ast::WindowType::WindowSpec(spec)
        }
        named @ ast::WindowType::NamedWindow(_) => named,
    };

    Ok((translated, warnings))
}

/// Build a `CONCAT(left, right)` function call expression.
fn make_concat_call(left: Expr, right: Expr) -> Expr {
    Expr::Function(ast::Function {
        name: ObjectName(vec![Ident::new("CONCAT")]),
        args: FunctionArguments::List(
            ast::FunctionArgumentList {
                duplicate_treatment: None,
                args: vec![
                    FunctionArg::Unnamed(
                        FunctionArgExpr::Expr(left),
                    ),
                    FunctionArg::Unnamed(
                        FunctionArgExpr::Expr(right),
                    ),
                ],
                clauses: vec![],
            },
        ),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
        parameters: FunctionArguments::None,
    })
}

/// Wrap an expression in a `LOWER()` function call.
fn wrap_in_lower(expr: Expr) -> Expr {
    Expr::Function(ast::Function {
        name: ObjectName(vec![Ident::new("LOWER")]),
        args: FunctionArguments::List(
            ast::FunctionArgumentList {
                duplicate_treatment: None,
                args: vec![FunctionArg::Unnamed(
                    FunctionArgExpr::Expr(expr),
                )],
                clauses: vec![],
            },
        ),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
        parameters: FunctionArguments::None,
    })
}
