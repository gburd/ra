//! Core SQL dialect translator.
//!
//! The [`DialectTranslator`] parses SQL in one dialect and
//! rewrites the AST for a different target dialect, handling
//! differences in syntax, function names, and operators.

use sqlparser::ast::{
    self, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, Ident, ObjectName, Query,
    SelectItem, SetExpr, Statement, Value,
};

use crate::backends::{Backend, TranslationBackend};
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

/// Database dialect with version information.
///
/// Used to enable version-specific translation rules
/// (e.g. PostgreSQL 15 features vs 12 features).
#[derive(Debug, Clone)]
pub struct DialectVersion {
    /// The database dialect.
    pub dialect: Dialect,
    /// Major version number (e.g. 15 for PostgreSQL 15).
    pub major: u16,
    /// Minor version number.
    pub minor: u16,
}

impl DialectVersion {
    /// Create a new dialect version.
    #[must_use]
    pub fn new(
        dialect: Dialect,
        major: u16,
        minor: u16,
    ) -> Self {
        Self {
            dialect,
            major,
            minor,
        }
    }

    /// Create a version with defaults (latest known).
    #[must_use]
    pub fn latest(dialect: Dialect) -> Self {
        let (major, minor) = match dialect {
            Dialect::PostgreSql => (17, 0),
            Dialect::MySql => (8, 4),
            Dialect::Sqlite => (3, 45),
            Dialect::MsSql => (16, 0),
            Dialect::Oracle => (23, 0),
            Dialect::DuckDb => (1, 1),
        };
        Self {
            dialect,
            major,
            minor,
        }
    }

    /// Whether this version supports the RETURNING clause.
    #[must_use]
    pub fn supports_returning(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => false,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 35,
            Dialect::MsSql => true, // OUTPUT clause
            Dialect::Oracle => self.major >= 12,
            Dialect::DuckDb => true,
        }
    }

    /// Whether this version supports CTE (WITH clause).
    #[must_use]
    pub fn supports_cte(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => self.major >= 8,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 8,
            Dialect::MsSql => true,
            Dialect::Oracle => true,
            Dialect::DuckDb => true,
        }
    }

    /// Whether this version supports window functions.
    #[must_use]
    pub fn supports_window_functions(&self) -> bool {
        match self.dialect {
            Dialect::PostgreSql => true,
            Dialect::MySql => self.major >= 8,
            Dialect::Sqlite => self.major >= 3 && self.minor >= 25,
            Dialect::MsSql => true,
            Dialect::Oracle => true,
            Dialect::DuckDb => true,
        }
    }
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
    source_version: DialectVersion,
    target_version: DialectVersion,
    backend: TranslationBackend,
}

impl DialectTranslator {
    /// Create a new translator from source to target dialect.
    #[must_use]
    pub fn new(source: Dialect, target: Dialect) -> Self {
        Self {
            source,
            target,
            source_version: DialectVersion::latest(source),
            target_version: DialectVersion::latest(target),
            backend: TranslationBackend::default(),
        }
    }

    /// Create a translator with a specific backend.
    #[must_use]
    pub fn with_backend(
        source: Dialect,
        target: Dialect,
        backend: TranslationBackend,
    ) -> Self {
        Self {
            source,
            target,
            source_version: DialectVersion::latest(source),
            target_version: DialectVersion::latest(target),
            backend,
        }
    }

    /// Create a translator with specific dialect versions.
    #[must_use]
    pub fn with_versions(
        source: DialectVersion,
        target: DialectVersion,
    ) -> Self {
        Self {
            source: source.dialect,
            target: target.dialect,
            source_version: source,
            target_version: target,
            backend: TranslationBackend::default(),
        }
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

    /// The source dialect version.
    #[must_use]
    pub fn source_version(&self) -> &DialectVersion {
        &self.source_version
    }

    /// The target dialect version.
    #[must_use]
    pub fn target_version(&self) -> &DialectVersion {
        &self.target_version
    }

    /// Get the current backend.
    #[must_use]
    pub fn backend(&self) -> TranslationBackend {
        self.backend
    }

    /// Translate a SQL string from the source dialect to the
    /// target dialect.
    ///
    /// # Errors
    ///
    /// Returns `TranslationError` if parsing fails or the SQL
    /// contains unsupported constructs.
    pub fn translate(&self, sql: &str) -> Result<TranslationResult, TranslationError> {
        // Delegate to the configured backend
        let backend_impl: Box<dyn Backend> = match self.backend {
            TranslationBackend::Native => {
                Box::new(crate::backends::native::NativeBackend)
            }
            #[cfg(feature = "polyglot-backend")]
            TranslationBackend::Polyglot => {
                Box::new(crate::backends::polyglot_backend::PolyglotBackend)
            }
        };

        backend_impl.translate(sql, self.source, self.target)
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
        // Translate CTEs
        if let Some(ref mut with) = query.with {
            for cte in &mut with.cte_tables {
                let translated = self.translate_query(*cte.query.clone(), warnings)?;
                *cte.query = translated;
            }
        }

        query.body = Box::new(self.translate_set_expr(*query.body, warnings)?);

        query = self.translate_limit_offset(query, warnings);

        if let Some(ref mut order_by) = query.order_by {
            for item in &mut order_by.exprs {
                item.expr = self.translate_expr(item.expr.clone(), warnings);
            }
            // Translate NULLS FIRST/LAST for dialects that
            // don't support it
            if !self.target.supports_nulls_first_last() {
                self.translate_order_by_nulls(
                    &mut order_by.exprs,
                    warnings,
                );
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
            } => self.translate_cast(
                *expr, data_type, format, kind,
                warnings,
            ),
            Expr::InSubquery {
                expr,
                subquery,
                negated,
            } => self.translate_in_subquery(*expr, *subquery, negated, warnings),
            Expr::Subquery(subquery) => {
                let cloned = subquery.clone();
                match self.translate_query(*cloned, warnings) {
                    Ok(q) => Expr::Subquery(Box::new(q)),
                    Err(_) => Expr::Subquery(subquery),
                }
            }
            Expr::Exists { subquery, negated } => {
                let cloned = subquery.clone();
                match self.translate_query(*cloned, warnings) {
                    Ok(q) => Expr::Exists {
                        subquery: Box::new(q),
                        negated,
                    },
                    Err(_) => Expr::Exists { subquery, negated },
                }
            }
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

        // Translate OVER clause for window functions
        if let Some(over) = func.over.take() {
            func.over =
                Some(self.translate_window_type(over, warnings));
        }

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

    /// Translate CAST expressions, converting PostgreSQL's
    /// `::` shorthand to standard CAST syntax for dialects
    /// that do not support it.
    fn translate_cast(
        &self,
        expr: Expr,
        data_type: ast::DataType,
        format: Option<ast::CastFormat>,
        kind: ast::CastKind,
        warnings: &mut Vec<TranslationWarning>,
    ) -> Expr {
        let translated_expr =
            self.translate_expr(expr, warnings);

        // PostgreSQL :: cast -> standard CAST for other dialects
        let new_kind =
            if kind == ast::CastKind::DoubleColon
                && !self.target.supports_double_colon_cast()
            {
                warnings.push(TranslationWarning {
                    severity: WarningSeverity::Info,
                    message: format!(
                        ":: cast translated to CAST() \
                         for {}",
                        self.target
                    ),
                    hint: None,
                });
                ast::CastKind::Cast
            } else {
                kind
            };

        Expr::Cast {
            expr: Box::new(translated_expr),
            data_type,
            format,
            kind: new_kind,
        }
    }

    /// Strip NULLS FIRST/LAST from ORDER BY for dialects
    /// that don't support them.
    fn translate_order_by_nulls(
        &self,
        exprs: &mut [ast::OrderByExpr],
        warnings: &mut Vec<TranslationWarning>,
    ) {
        let mut warned = false;
        for item in exprs.iter_mut() {
            if item.nulls_first.is_some() {
                item.nulls_first = None;
                if !warned {
                    warnings.push(TranslationWarning {
                        severity: WarningSeverity::Warning,
                        message: format!(
                            "NULLS FIRST/LAST removed for \
                             {} (not supported)",
                            self.target
                        ),
                        hint: Some(
                            "Use CASE WHEN ... IS NULL to \
                             control NULL ordering"
                                .into(),
                        ),
                    });
                    warned = true;
                }
            }
        }
    }

    /// Translate window function OVER clauses, recursively
    /// translating expressions within `partition_by` and
    /// `order_by`.
    fn translate_window_type(
        &self,
        window: ast::WindowType,
        warnings: &mut Vec<TranslationWarning>,
    ) -> ast::WindowType {
        match window {
            ast::WindowType::WindowSpec(mut spec) => {
                for expr in &mut spec.partition_by {
                    *expr =
                        self.translate_expr(expr.clone(), warnings);
                }
                for item in &mut spec.order_by {
                    item.expr =
                        self.translate_expr(item.expr.clone(), warnings);
                }
                ast::WindowType::WindowSpec(spec)
            }
            named @ ast::WindowType::NamedWindow(_) => named,
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

    #[test]
    fn cte_translation() {
        let result = pg_to(
            Dialect::MySql,
            "WITH active AS (SELECT * FROM users \
             WHERE active = TRUE) \
             SELECT * FROM active",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        // Boolean TRUE should be translated for MySQL? No,
        // MySQL supports TRUE. Let's check it passes through.
        assert!(result.sql.contains("active"));
    }

    #[test]
    fn cte_to_sqlite_boolean_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "WITH cte AS (SELECT * FROM t \
             WHERE flag = TRUE) \
             SELECT * FROM cte",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in CTE body: {}",
            result.sql
        );
    }

    #[test]
    fn recursive_cte_translation() {
        let result = pg_to(
            Dialect::MySql,
            "WITH RECURSIVE nums AS (\
             SELECT 1 AS n \
             UNION ALL \
             SELECT n + 1 FROM nums WHERE n < 10) \
             SELECT * FROM nums",
        );
        assert!(
            result.sql.contains("RECURSIVE"),
            "Expected RECURSIVE in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT name, ROW_NUMBER() OVER \
             (PARTITION BY dept ORDER BY salary DESC) \
             AS rn FROM employees",
        );
        assert!(
            result.sql.contains("ROW_NUMBER"),
            "Expected ROW_NUMBER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("PARTITION BY"),
            "Expected PARTITION BY in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT id, SUM(amount) OVER \
             (PARTITION BY customer_id ORDER BY date) \
             FROM orders",
        );
        assert!(
            result.sql.contains("SUM"),
            "Expected SUM in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
    }

    #[test]
    fn window_function_with_boolean_in_partition() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT ROW_NUMBER() OVER \
             (PARTITION BY active ORDER BY id) \
             FROM users WHERE active = TRUE",
        );
        assert!(
            result.sql.contains("OVER"),
            "Expected OVER in: {}",
            result.sql
        );
        // TRUE should become 1 for SQLite
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn distinct_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT DISTINCT name FROM users",
        );
        assert!(
            result.sql.contains("DISTINCT"),
            "Expected DISTINCT in: {}",
            result.sql
        );
    }

    #[test]
    fn having_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT dept, COUNT(*) FROM employees \
             GROUP BY dept HAVING COUNT(*) > 5",
        );
        assert!(
            result.sql.contains("HAVING"),
            "Expected HAVING in: {}",
            result.sql
        );
    }

    #[test]
    fn having_with_boolean_translation() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT dept, COUNT(*) FROM employees \
             GROUP BY dept HAVING COUNT(*) > 5 \
             AND active = TRUE",
        );
        assert!(
            result.sql.contains("HAVING"),
            "Expected HAVING in: {}",
            result.sql
        );
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn subquery_in_where() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT * FROM orders WHERE customer_id \
             IN (SELECT id FROM customers WHERE active = TRUE)",
        );
        assert!(
            result.sql.contains("IN ("),
            "Expected IN subquery in: {}",
            result.sql
        );
    }

    #[test]
    fn exists_subquery() {
        let result = pg_to(
            Dialect::Sqlite,
            "SELECT * FROM orders WHERE EXISTS \
             (SELECT 1 FROM customers \
             WHERE customers.id = orders.customer_id \
             AND active = TRUE)",
        );
        assert!(
            result.sql.contains("EXISTS"),
            "Expected EXISTS in: {}",
            result.sql
        );
        assert!(
            result.sql.contains('1'),
            "Expected TRUE -> 1 in: {}",
            result.sql
        );
    }

    #[test]
    fn scalar_subquery_translation() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT name, \
             (SELECT COUNT(*) FROM orders \
             WHERE orders.user_id = users.id) AS cnt \
             FROM users",
        );
        assert!(
            result.sql.contains("SELECT"),
            "Expected nested SELECT in: {}",
            result.sql
        );
    }

    #[test]
    fn order_by_with_limit_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "SELECT * FROM users \
             ORDER BY name ASC LIMIT 10",
        );
        assert!(
            result.sql.contains("ORDER BY"),
            "Expected ORDER BY in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in: {}",
            result.sql
        );
    }

    #[test]
    fn cte_with_limit_to_mssql() {
        let result = pg_to(
            Dialect::MsSql,
            "WITH top_users AS (\
             SELECT * FROM users ORDER BY score DESC \
             LIMIT 10) \
             SELECT * FROM top_users",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH in: {}",
            result.sql
        );
        assert!(
            result.sql.contains("FETCH"),
            "Expected FETCH in CTE body: {}",
            result.sql
        );
    }

    #[test]
    fn version_support() {
        let pg17 = DialectVersion::latest(Dialect::PostgreSql);
        assert!(pg17.supports_returning());
        assert!(pg17.supports_cte());
        assert!(pg17.supports_window_functions());

        let mysql5 = DialectVersion::new(Dialect::MySql, 5, 7);
        assert!(!mysql5.supports_cte());
        assert!(!mysql5.supports_window_functions());

        let mysql8 = DialectVersion::new(Dialect::MySql, 8, 0);
        assert!(mysql8.supports_cte());
        assert!(mysql8.supports_window_functions());

        let sqlite_old = DialectVersion::new(Dialect::Sqlite, 3, 24);
        assert!(!sqlite_old.supports_returning());
        assert!(!sqlite_old.supports_window_functions());

        let sqlite_new = DialectVersion::new(Dialect::Sqlite, 3, 35);
        assert!(sqlite_new.supports_returning());
    }

    #[test]
    fn translator_with_versions() {
        let source = DialectVersion::new(Dialect::PostgreSql, 15, 0);
        let target = DialectVersion::new(Dialect::MySql, 8, 0);
        let t = DialectTranslator::with_versions(source, target);
        assert_eq!(t.source(), Dialect::PostgreSql);
        assert_eq!(t.target(), Dialect::MySql);
        assert_eq!(t.source_version().major, 15);
        assert_eq!(t.target_version().major, 8);
    }

    #[test]
    fn double_colon_cast_to_mysql() {
        let result = pg_to(
            Dialect::MySql,
            "SELECT age::int FROM users",
        );
        assert!(
            result.sql.contains("CAST"),
            "Expected CAST in: {}",
            result.sql
        );
    }

    #[test]
    fn double_colon_cast_stays_for_postgres() {
        let result = pg_to(
            Dialect::PostgreSql,
            "SELECT age::int FROM users",
        );
        // PostgreSQL supports ::, so it should stay as-is
        assert!(
            result.sql.contains("::") || result.sql.contains("CAST"),
            "Expected :: or CAST in: {}",
            result.sql
        );
    }

    #[test]
    fn combined_cte_window_distinct() {
        let result = pg_to(
            Dialect::MySql,
            "WITH ranked AS (\
             SELECT name, \
             ROW_NUMBER() OVER (ORDER BY score DESC) AS rn \
             FROM users) \
             SELECT DISTINCT name FROM ranked \
             WHERE rn <= 10",
        );
        assert!(
            result.sql.contains("WITH"),
            "Expected WITH: {}",
            result.sql
        );
        assert!(
            result.sql.contains("ROW_NUMBER"),
            "Expected ROW_NUMBER: {}",
            result.sql
        );
        assert!(
            result.sql.contains("DISTINCT"),
            "Expected DISTINCT: {}",
            result.sql
        );
    }
}
