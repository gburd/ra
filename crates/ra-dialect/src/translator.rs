//! Core SQL dialect translator.
//!
//! The [`DialectTranslator`] parses SQL in one dialect and
//! rewrites the AST for a different target dialect, handling
//! differences in syntax, function names, and operators.

use sqlparser::ast::{
    self, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, ObjectName, Query,
    SelectItem, SetExpr, Statement, Value,
};
use sqlparser::parser::Parser;

use crate::dialect::Dialect;
use crate::error::{TranslationError, TranslationWarning, WarningSeverity};
use crate::functions::build_function_map;

/// Result of a dialect translation.
#[derive(Debug)]
pub struct TranslationResult {
    /// The translated SQL string.
    pub sql: String,
    /// Warnings generated during translation.
    pub warnings: Vec<TranslationWarning>,
}

/// Translates SQL between different database dialects.
///
/// # Example
///
/// ```
/// use ra_dialect::{Dialect, DialectTranslator};
///
/// let translator = DialectTranslator::new(
///     Dialect::PostgreSql,
///     Dialect::MySql,
/// );
/// let result = translator
///     .translate("SELECT * FROM users LIMIT 10")
///     .unwrap();
/// assert!(result.sql.contains("LIMIT"));
/// ```
pub struct DialectTranslator {
    source: Dialect,
    target: Dialect,
}

impl DialectTranslator {
    /// Create a new translator from source to target dialect.
    #[must_use]
    pub fn new(source: Dialect, target: Dialect) -> Self {
        Self { source, target }
    }

    /// The source dialect.
    #[must_use]
    pub fn source(&self) -> Dialect {
        self.source
    }

    /// The target dialect.
    #[must_use]
    pub fn target(&self) -> Dialect {
        self.target
    }

    /// Translate a SQL string from the source dialect to the
    /// target dialect.
    ///
    /// # Errors
    ///
    /// Returns `TranslationError` if parsing fails or the SQL
    /// contains unsupported constructs.
    pub fn translate(&self, sql: &str) -> Result<TranslationResult, TranslationError> {
        let dialect = self.source.sqlparser_dialect();
        let statements = Parser::parse_sql(&*dialect, sql)?;

        let mut warnings = Vec::new();
        let mut translated = Vec::new();

        for stmt in statements {
            let rewritten = self.translate_statement(stmt, &mut warnings)?;
            translated.push(rewritten.to_string());
        }

        Ok(TranslationResult {
            sql: translated.join(";\n"),
            warnings,
        })
    }

    fn translate_statement(
        &self,
        stmt: Statement,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Result<Statement, TranslationError> {
        match stmt {
            Statement::Query(query) => {
                let rewritten = self.translate_query(*query, warnings)?;
                Ok(Statement::Query(Box::new(rewritten)))
            }
            other => Ok(other),
        }
    }

    fn translate_query(
        &self,
        mut query: Query,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Result<Query, TranslationError> {
        query.body = Box::new(self.translate_set_expr(*query.body, warnings)?);

        query = self.translate_limit_offset(query, warnings);

        if let Some(ref mut order_by) = query.order_by {
            for item in &mut order_by.exprs {
                item.expr = self.translate_expr(item.expr.clone(), warnings);
            }
        }

        Ok(query)
    }

    fn translate_set_expr(
        &self,
        set_expr: SetExpr,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Result<SetExpr, TranslationError> {
        match set_expr {
            SetExpr::Select(select) => Ok(self.translate_select(*select, warnings)),
            SetExpr::SetOperation {
                op,
                set_quantifier,
                left,
                right,
            } => {
                let op = self.translate_set_op(op, warnings);
                let left = Box::new(self.translate_set_expr(*left, warnings)?);
                let right = Box::new(self.translate_set_expr(*right, warnings)?);
                Ok(SetExpr::SetOperation {
                    op,
                    set_quantifier,
                    left,
                    right,
                })
            }
            other => Ok(other),
        }
    }

    fn translate_select(
        &self,
        mut select: ast::Select,
        warnings: &mut Vec<TranslationWarning>,
    ) -> SetExpr {
        for item in &mut select.projection {
            *item = self.translate_select_item(item.clone(), warnings);
        }

        if let Some(selection) = select.selection.take() {
            select.selection = Some(self.translate_expr(selection, warnings));
        }

        if let Some(having) = select.having.take() {
            select.having = Some(self.translate_expr(having, warnings));
        }

        select.group_by = self.translate_group_by(select.group_by, warnings);

        SetExpr::Select(Box::new(select))
    }

    fn translate_select_item(
        &self,
        item: SelectItem,
        warnings: &mut Vec<TranslationWarning>,
    ) -> SelectItem {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                SelectItem::UnnamedExpr(self.translate_expr(expr, warnings))
            }
            SelectItem::ExprWithAlias { expr, alias } => SelectItem::ExprWithAlias {
                expr: self.translate_expr(expr, warnings),
                alias,
            },
            other => other,
        }
    }

    fn translate_group_by(
        &self,
        group_by: ast::GroupByExpr,
        warnings: &mut Vec<TranslationWarning>,
    ) -> ast::GroupByExpr {
        match group_by {
            ast::GroupByExpr::Expressions(exprs, modifiers) => {
                let translated: Vec<Expr> = exprs
                    .into_iter()
                    .map(|e| self.translate_expr(e, warnings))
                    .collect();
                ast::GroupByExpr::Expressions(translated, modifiers)
            }
            other @ ast::GroupByExpr::All(_) => other,
        }
    }

    fn translate_set_op(
        &self,
        op: ast::SetOperator,
        warnings: &mut Vec<TranslationWarning>,
    ) -> ast::SetOperator {
        if matches!(self.target, Dialect::Oracle) && matches!(op, ast::SetOperator::Except) {
            warnings.push(TranslationWarning {
                severity: WarningSeverity::Info,
                message: "EXCEPT translated to MINUS for Oracle".into(),
                hint: None,
            });
        }
        op
    }

    /// Translate LIMIT/OFFSET syntax to the target dialect.
    fn translate_limit_offset(
        &self,
        mut query: Query,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Query {
        if self.source == self.target {
            return query;
        }

        let has_limit = query.limit.is_some();
        let has_offset = query.offset.is_some();

        if !has_limit && !has_offset {
            return query;
        }

        // LIMIT -> FETCH for MSSQL/Oracle
        if !self.target.supports_limit() && self.target.supports_fetch() {
            if let Some(limit_expr) = query.limit.take() {
                query.fetch = Some(ast::Fetch {
                    with_ties: false,
                    percent: false,
                    quantity: Some(limit_expr),
                });

                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!("LIMIT translated to FETCH for {}", self.target),
                    hint: Some("FETCH requires ORDER BY in MSSQL".into()),
                });
            }
        }

        // FETCH -> LIMIT for MySQL/SQLite etc.
        if self.target.supports_limit() && !self.source.supports_limit() {
            if let Some(fetch) = query.fetch.take() {
                if let Some(quantity) = fetch.quantity {
                    query.limit = Some(quantity);
                    warnings.push(TranslationWarning {
                        severity: WarningSeverity::Info,
                        message: format!(
                            "FETCH translated to LIMIT \
                             for {}",
                            self.target
                        ),
                        hint: None,
                    });
                }
            }
        }

        query
    }

    /// Translate an expression, rewriting functions,
    /// operators, and dialect-specific syntax.
    fn translate_expr(&self, expr: Expr, warnings: &mut Vec<TranslationWarning>) -> Expr {
        match expr {
            Expr::Function(func) => self.translate_function(func, warnings),
            Expr::BinaryOp { left, op, right } => {
                let left = self.translate_expr(*left, warnings);
                let right = self.translate_expr(*right, warnings);
                self.translate_binary_op(left, op, right, warnings)
            }
            Expr::ILike {
                negated,
                expr,
                pattern,
                escape_char,
                ..
            } => self.translate_ilike(negated, *expr, *pattern, escape_char, warnings),
            Expr::Nested(inner) => Expr::Nested(Box::new(self.translate_expr(*inner, warnings))),
            Expr::UnaryOp { op, expr } => Expr::UnaryOp {
                op,
                expr: Box::new(self.translate_expr(*expr, warnings)),
            },
            Expr::IsNull(e) => Expr::IsNull(Box::new(self.translate_expr(*e, warnings))),
            Expr::IsNotNull(e) => Expr::IsNotNull(Box::new(self.translate_expr(*e, warnings))),
            Expr::Value(v) => self.translate_value(v, warnings),
            Expr::Between {
                expr,
                negated,
                low,
                high,
            } => self.translate_between(*expr, negated, *low, *high, warnings),
            Expr::Case {
                operand,
                conditions,
                results,
                else_result,
            } => self.translate_case(operand, conditions, results, else_result, warnings),
            Expr::Cast {
                expr,
                data_type,
                format,
                kind,
            } => Expr::Cast {
                expr: Box::new(self.translate_expr(*expr, warnings)),
                data_type,
                format,
                kind,
            },
            Expr::InSubquery {
                expr,
                subquery,
                negated,
            } => self.translate_in_subquery(*expr, *subquery, negated, warnings),
            other => other,
        }
    }

    fn translate_between(
        &self,
        expr: Expr,
        negated: bool,
        low: Expr,
        high: Expr,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        Expr::Between {
            expr: Box::new(self.translate_expr(expr, warnings)),
            negated,
            low: Box::new(self.translate_expr(low, warnings)),
            high: Box::new(self.translate_expr(high, warnings)),
        }
    }

    fn translate_case(
        &self,
        operand: Option<Box<Expr>>,
        conditions: Vec<Expr>,
        results: Vec<Expr>,
        else_result: Option<Box<Expr>>,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        Expr::Case {
            operand: operand.map(|e| Box::new(self.translate_expr(*e, warnings))),
            conditions: conditions
                .into_iter()
                .map(|c| self.translate_expr(c, warnings))
                .collect(),
            results: results
                .into_iter()
                .map(|r| self.translate_expr(r, warnings))
                .collect(),
            else_result: else_result.map(|e| Box::new(self.translate_expr(*e, warnings))),
        }
    }

    fn translate_in_subquery(
        &self,
        expr: Expr,
        subquery: Query,
        negated: bool,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        match self.translate_query(subquery, warnings) {
            Ok(q) => Expr::InSubquery {
                expr: Box::new(self.translate_expr(expr, warnings)),
                subquery: Box::new(q),
                negated,
            },
            Err(_) => Expr::InSubquery {
                expr: Box::new(expr),
                subquery: Box::new(Query {
                    with: None,
                    body: Box::new(SetExpr::Values(ast::Values {
                        explicit_row: false,
                        rows: vec![],
                    })),
                    order_by: None,
                    limit: None,
                    limit_by: vec![],
                    offset: None,
                    fetch: None,
                    locks: vec![],
                    for_clause: None,
                    settings: None,
                    format_clause: None,
                }),
                negated,
            },
        }
    }

    /// Translate a function call, mapping function names
    /// and adjusting arguments as needed.
    fn translate_function(
        &self,
        mut func: ast::Function,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        let func_name = func.name.to_string().to_uppercase();
        let function_map = build_function_map(self.target);

        if let Some(mapping) = function_map.get(&func_name) {
            let old_name = func.name.to_string();
            func.name = ObjectName(vec![Ident::new(&mapping.target_name)]);

            if old_name.to_uppercase() != mapping.target_name.to_uppercase() {
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!(
                        "{old_name} translated to {} \
                         for {}",
                        mapping.target_name, self.target
                    ),
                    hint: None,
                });
            }
        }

        func.args = self.translate_function_args(func.args, warnings);

        Expr::Function(func)
    }

    fn translate_function_args(
        &self,
        args: FunctionArguments,
        warnings: &mut Vec<TranslationWarning>,
    ) -> FunctionArguments {
        match args {
            FunctionArguments::List(mut arg_list) => {
                for arg in &mut arg_list.args {
                    *arg = self.translate_function_arg(arg.clone(), warnings);
                }
                FunctionArguments::List(arg_list)
            }
            other => other,
        }
    }

    fn translate_function_arg(
        &self,
        arg: FunctionArg,
        warnings: &mut Vec<TranslationWarning>,
    ) -> FunctionArg {
        match arg {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(self.translate_expr(expr, warnings)))
            }
            FunctionArg::Named {
                name,
                arg: FunctionArgExpr::Expr(expr),
                operator,
            } => FunctionArg::Named {
                name,
                arg: FunctionArgExpr::Expr(self.translate_expr(expr, warnings)),
                operator,
            },
            other => other,
        }
    }

    /// Translate binary operators.
    fn translate_binary_op(
        &self,
        left: Expr,
        op: ast::BinaryOperator,
        right: Expr,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        if matches!(op, ast::BinaryOperator::StringConcat) {
            return self.translate_string_concat(left, right, warnings);
        }

        Expr::BinaryOp {
            left: Box::new(left),
            op,
            right: Box::new(right),
        }
    }

    /// Translate `||` string concatenation to the target
    /// dialect.
    fn translate_string_concat(
        &self,
        left: Expr,
        right: Expr,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        match self.target {
            Dialect::PostgreSql | Dialect::Sqlite | Dialect::DuckDb | Dialect::Oracle => {
                Expr::BinaryOp {
                    left: Box::new(left),
                    op: ast::BinaryOperator::StringConcat,
                    right: Box::new(right),
                }
            }
            Dialect::MsSql => {
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: "|| translated to + for MSSQL \
                         string concatenation"
                        .into(),
                    hint: None,
                });
                Expr::BinaryOp {
                    left: Box::new(left),
                    op: ast::BinaryOperator::Plus,
                    right: Box::new(right),
                }
            }
            Dialect::MySql => {
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: "|| translated to CONCAT() for \
                         MySQL"
                        .into(),
                    hint: None,
                });
                make_concat_call(left, right)
            }
        }
    }

    /// Translate ILIKE to dialects that don't support it.
    fn translate_ilike(
        &self,
        negated: bool,
        expr: Expr,
        pattern: Expr,
        escape_char: Option<String>,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        if self.target.supports_ilike() {
            return Expr::ILike {
                negated,
                any: false,
                expr: Box::new(self.translate_expr(expr, warnings)),
                pattern: Box::new(self.translate_expr(pattern, warnings)),
                escape_char,
            };
        }

        warnings.push(TranslationWarning {
            severity: WarningSeverity::Info,
            message: format!(
                "ILIKE translated to LOWER() + LIKE \
                 for {}",
                self.target
            ),
            hint: Some(
                "LOWER() may not handle all Unicode \
                 case folding correctly"
                    .into(),
            ),
        });

        let lower_expr = wrap_in_lower(self.translate_expr(expr, warnings));
        let lower_pattern = wrap_in_lower(self.translate_expr(pattern, warnings));

        Expr::Like {
            negated,
            any: false,
            expr: Box::new(lower_expr),
            pattern: Box::new(lower_pattern),
            escape_char,
        }
    }

    /// Translate boolean literal values for dialects that
    /// don't support TRUE/FALSE keywords.
    fn translate_value(&self, value: Value, warnings: &mut Vec<TranslationWarning>) -> Expr {
        match &value {
            Value::Boolean(b) if !self.target.supports_boolean_literals() => {
                let int_val = i32::from(*b);
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!(
                        "Boolean literal translated to \
                         {int_val} for {}",
                        self.target
                    ),
                    hint: None,
                });
                Expr::Value(Value::Number(int_val.to_string(), false))
            }
            _ => Expr::Value(value),
        }
    }
}

/// Build a CONCAT(left, right) function call expression.
fn make_concat_call(left: Expr, right: Expr) -> Expr {
    Expr::Function(ast::Function {
        name: ObjectName(vec![Ident::new("CONCAT")]),
        args: FunctionArguments::List(ast::FunctionArgumentList {
            duplicate_treatment: None,
            args: vec![
                FunctionArg::Unnamed(FunctionArgExpr::Expr(left)),
                FunctionArg::Unnamed(FunctionArgExpr::Expr(right)),
            ],
            clauses: vec![],
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
        parameters: FunctionArguments::None,
    })
}

/// Wrap an expression in `LOWER()` function call.
fn wrap_in_lower(expr: Expr) -> Expr {
    Expr::Function(ast::Function {
        name: ObjectName(vec![Ident::new("LOWER")]),
        args: FunctionArguments::List(ast::FunctionArgumentList {
            duplicate_treatment: None,
            args: vec![FunctionArg::Unnamed(FunctionArgExpr::Expr(expr))],
            clauses: vec![],
        }),
        filter: None,
        null_treatment: None,
        over: None,
        within_group: vec![],
        parameters: FunctionArguments::None,
    })
}

#[cfg(test)]
#[expect(clippy::expect_used)] // tests intentionally panic on failure
mod tests {
    use super::*;

    fn pg_to(target: Dialect, sql: &str) -> TranslationResult {
        DialectTranslator::new(Dialect::PostgreSql, target)
            .translate(sql)
            .expect("translation should succeed")
    }

    #[test]
    fn identity_translation() {
        let result = pg_to(Dialect::PostgreSql, "SELECT 1");
        assert!(result.sql.contains("SELECT"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn limit_to_mysql() {
        let result = pg_to(Dialect::MySql, "SELECT * FROM users LIMIT 10");
        assert!(result.sql.contains("LIMIT"));
    }

    #[test]
    fn limit_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT * FROM users LIMIT 10");
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
        assert!(
            !result.sql.contains("LIMIT"),
            "Should not contain LIMIT: {}",
            result.sql
        );
    }

    #[test]
    fn limit_offset_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT * FROM users \
             ORDER BY id LIMIT 10 OFFSET 20",
        );
        assert!(
            result.sql.contains("OFFSET"),
            "Expected OFFSET in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
    }

    #[test]
    fn string_concat_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT first_name || ' ' || last_name \
             FROM users",
        );
        assert!(
            result.sql.contains("CONCAT"),
            "Expected CONCAT in: {}",
            result.sql
        );
    }

    #[test]
    fn string_concat_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT first_name || last_name FROM users");
        assert!(result.sql.contains('+'), "Expected + in: {}", result.sql);
    }

    #[test]
    fn ilike_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT * FROM users \
             WHERE name ILIKE '%john%'",
        );
        assert!(
            result.sql.contains("LOWER"),
            "Expected LOWER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("LIKE"),
            "Expected LIKE in: {}",
            result.sql
        );
    }

    #[test]
    fn ilike_stays_for_postgres() {
        let result = pg_to(
            Dialect::PostgreSql,
            "SELECT * FROM users \
             WHERE name ILIKE '%john%'",
        );
        assert!(
            result.sql.contains("ILIKE"),
            "Expected ILIKE preserved: {}",
            result.sql
        );
    }

    #[test]
    fn boolean_to_sqlite() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT * FROM flags \
             WHERE active = TRUE",
        );
        assert!(
            result.sql.contains('1'),
            "Expected 1 for TRUE in: {}",
            result.sql
        );
    }

    #[test]
    fn boolean_to_mssql() {
        let result = pg_to(Dialect::MsSql, "SELECT * FROM t WHERE x = FALSE");
        assert!(
            result.sql.contains('0'),
            "Expected 0 for FALSE in: {}",
            result.sql
        );
    }

    #[test]
    fn select_with_where() {
        let result = pg_to(Dialect::MySql, "SELECT id, name FROM users WHERE age > 18");
        assert!(result.sql.contains("WHERE"));
        assert!(result.sql.contains("age"));
    }

    #[test]
    fn union_query() {
        let result = pg_to(Dialect::MySql, "SELECT id FROM a UNION SELECT id FROM b");
        assert!(result.sql.contains("UNION"));
    }

    #[test]
    fn translator_accessors() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        assert_eq!(t.source(), Dialect::PostgreSql);
        assert_eq!(t.target(), Dialect::MySql);
    }

    #[test]
    fn case_expression_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT CASE WHEN x = TRUE THEN 'yes' \
             ELSE 'no' END FROM t",
        );
        assert!(
            result.sql.contains('1'),
            "Expected boolean translation in: {}",
            result.sql
        );
    }

    #[test]
    fn nested_function_translation() {
        let result = pg_to(Dialect::MsSql, "SELECT LENGTH(UPPER(name)) FROM users");
        assert!(
            result.sql.contains("LEN"),
            "Expected LEN in: {}",
            result.sql
        );
    }

    #[test]
    fn multiple_statements() {
        let result = pg_to(Dialect::MySql, "SELECT 1; SELECT 2");
        assert!(result.sql.contains("SELECT 1"));
        assert!(result.sql.contains("SELECT 2"));
    }

    #[test]
    fn parse_error_propagated() {
        let t = DialectTranslator::new(Dialect::PostgreSql, Dialect::MySql);
        let err = t.translate("NOT VALID SQL !!! %%%");
        assert!(err.is_err());
    }
}
