//! Native SQL dialect translation backend.
//!
//! This module contains the original hand-written translation
//! logic for the core 6 dialects.

use sqlparser::ast::{
    self, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, ObjectName, Query,
    SelectItem, SetExpr, Statement,
};
use sqlparser::parser::Parser;

use crate::dialect::Dialect;
use crate::error::{TranslationError, TranslationWarning, WarningSeverity};
use crate::functions::build_function_map;
use crate::{Backend, TranslationResult};

/// Native translation backend implementation.
pub struct NativeBackend;

impl Backend for NativeBackend {
    fn translate(
        &self,
        sql: &str,
        source: Dialect,
        target: Dialect,
    ) -> Result<TranslationResult, TranslationError> {
        // Parse the SQL using the source dialect
        let source_parser = source.sqlparser_dialect();
        let statements = Parser::parse_sql(&*source_parser, sql)?;

        let mut warnings = Vec::new();
        let mut translated_statements = Vec::new();

        for statement in statements {
            let (translated, mut stmt_warnings) =
                translate_statement(statement, source, target)?;
            warnings.append(&mut stmt_warnings);
            translated_statements.push(translated);
        }

        // Join all statements with semicolons
        let sql = translated_statements.join(";\n");

        Ok(TranslationResult { sql, warnings })
    }
}

/// Translate a single statement.
fn translate_statement(
    statement: Statement,
    source: Dialect,
    target: Dialect,
) -> Result<(String, Vec<TranslationWarning>), TranslationError> {
    let mut warnings = Vec::new();

    let translated = match statement {
        Statement::Query(query) => {
            let (translated_query, mut query_warnings) = translate_query(*query, source, target)?;
            warnings.append(&mut query_warnings);
            Statement::Query(Box::new(translated_query))
        }
        // For non-query statements, pass through for now
        // TODO: Add translation for INSERT, UPDATE, DELETE, CREATE, etc.
        _ => statement,
    };

    Ok((translated.to_string(), warnings))
}

/// Translate a query.
fn translate_query(
    mut query: Query,
    source: Dialect,
    target: Dialect,
) -> Result<(Query, Vec<TranslationWarning>), TranslationError> {
    let mut warnings = Vec::new();

    // Handle LIMIT/OFFSET translation
    if let Some(_limit) = &query.limit {
        if !target.supports_limit() {
            // Convert to FETCH for dialects that don't support LIMIT
            if target.supports_fetch() {
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!("LIMIT translated to FETCH for {target}"),
                    hint: None,
                });
                // TODO: Actually convert to FETCH syntax
            } else {
                return Err(TranslationError::UnsupportedFeature {
                    dialect: target,
                    feature: "LIMIT clause".to_string(),
                });
            }
        }
    }

    // Translate the query body
    if let SetExpr::Select(ref mut select) = &mut *query.body {
        // Translate function calls in SELECT items
        for item in &mut select.projection {
            if let SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } = item {
                let (translated_expr, mut expr_warnings) =
                    translate_expression(expr.clone(), source, target)?;
                warnings.append(&mut expr_warnings);
                *expr = translated_expr;
            }
        }

        // Translate WHERE clause
        if let Some(ref mut where_clause) = select.selection {
            let (translated_expr, mut expr_warnings) =
                translate_expression(where_clause.clone(), source, target)?;
            warnings.append(&mut expr_warnings);
            *where_clause = translated_expr;
        }
    }

    Ok((query, warnings))
}

/// Translate an expression.
fn translate_expression(
    expr: Expr,
    source: Dialect,
    target: Dialect,
) -> Result<(Expr, Vec<TranslationWarning>), TranslationError> {
    let mut warnings = Vec::new();

    let translated = match expr {
        // String concatenation
        Expr::BinaryOp { left, op, right } if matches!(op, ast::BinaryOperator::StringConcat) => {
            if target == Dialect::MySql || target == Dialect::MsSql {
                // MySQL and SQL Server use CONCAT() instead of ||
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!("String concatenation || translated to CONCAT() for {target}"),
                    hint: None,
                });

                Expr::Function(ast::Function {
                    name: ObjectName(vec![Ident::new("CONCAT")]),
                    args: FunctionArguments::List(ast::FunctionArgumentList {
                        duplicate_treatment: None,
                        args: vec![
                            FunctionArg::Unnamed(FunctionArgExpr::Expr(*left)),
                            FunctionArg::Unnamed(FunctionArgExpr::Expr(*right)),
                        ],
                        clauses: vec![],
                    }),
                    filter: None,
                    null_treatment: None,
                    over: None,
                    within_group: vec![],
                    parameters: FunctionArguments::None,
                })
            } else {
                Expr::BinaryOp { left, op, right }
            }
        }

        // Function translation
        Expr::Function(mut func) => {
            let function_map = build_function_map(target);
            let func_name = func.name.to_string().to_uppercase();

            if let Some(mapping) = function_map.get(&func_name) {
                let target_name = &mapping.target_name;
                if target_name != &func_name {
                    warnings.push(TranslationWarning {
                        severity: WarningSeverity::Info,
                        message: format!("Function {func_name} translated to {target_name}"),
                        hint: None,
                    });
                    func.name = ObjectName(vec![Ident::new(target_name)]);
                }
            }

            // Recursively translate function arguments
            if let FunctionArguments::List(ref mut args) = func.args {
                for arg in &mut args.args {
                    if let FunctionArg::Unnamed(FunctionArgExpr::Expr(ref mut arg_expr)) = arg {
                        let (translated_arg, mut arg_warnings) =
                            translate_expression(arg_expr.clone(), source, target)?;
                        warnings.append(&mut arg_warnings);
                        *arg_expr = translated_arg;
                    }
                }
            }

            Expr::Function(func)
        }

        // Recursively handle other expression types
        Expr::BinaryOp { left, op, right } => {
            let (translated_left, mut left_warnings) =
                translate_expression(*left, source, target)?;
            warnings.append(&mut left_warnings);

            let (translated_right, mut right_warnings) =
                translate_expression(*right, source, target)?;
            warnings.append(&mut right_warnings);

            Expr::BinaryOp {
                left: Box::new(translated_left),
                op,
                right: Box::new(translated_right),
            }
        }

        // Pass through other expressions unchanged
        _ => expr,
    };

    Ok((translated, warnings))
}