//! SQL emitter: converts `RelExpr` trees to dialect-specific SQL strings.
//!
//! The emitter walks a [`RelExpr`] tree and produces a SQL string
//! appropriate for the target [`Dialect`]. It handles dialect-specific
//! differences in syntax, function names, identifier quoting, boolean
//! literals, LIMIT/OFFSET, and string concatenation.

use std::fmt::Write;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering, ProjectionColumn, RelExpr,
    SortDirection, SortKey,
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
    /// Monotonic counter for unique derived-table aliases (`_sq0`, `_sq1`, …).
    alias_counter: usize,
}

impl EmitContext {
    fn new(target: Dialect) -> Self {
        Self {
            target,
            warnings: Vec::new(),
            function_map: build_function_map(target),
            alias_counter: 0,
        }
    }

    fn emit_rel_expr(&mut self, expr: &RelExpr) -> Result<String, TranslationError> {
        // Set operations and CTEs are the only top-level shapes that are *not*
        // a single flat SELECT; everything else flattens into one SELECT so
        // that base-table aliases stay in scope.
        match expr {
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
            RelExpr::Values { rows } => self.emit_values(rows),
            _ => {
                let sel = self.flatten(expr)?;
                self.render_select(&sel)
            }
        }
    }

    /// Flatten a Scan/Filter/Project/Join/Aggregate/Sort/Limit/Distinct/Window
    /// chain into a single `Select`, preserving each base table's original
    /// alias. Only genuinely non-flattenable inputs (set-ops, CTEs, an
    /// aggregate feeding another aggregate, etc.) are pushed down into a
    /// derived-table subquery via [`Self::source_from`].
    fn flatten(&mut self, expr: &RelExpr) -> Result<Select, TranslationError> {
        let mut sel = Select::default();
        self.flatten_into(expr, &mut sel)?;
        Ok(sel)
    }

    fn flatten_into(&mut self, expr: &RelExpr, sel: &mut Select) -> Result<(), TranslationError> {
        match expr {
            // A named derived table: `FROM (<input>) AS alias`. Emit the input
            // as a subquery carrying the REAL alias so outer `alias.col`
            // references resolve. It is an optimization/scope barrier, so it
            // becomes its own FROM item.
            RelExpr::SubqueryAlias { alias, input } => {
                let sql = self.emit_rel_expr(input)?;
                sel.from = Some(FromItem::Subquery {
                    sql,
                    alias: alias.clone(),
                });
                Ok(())
            }
            RelExpr::Project { columns, input } => {
                if sel.projection.is_some() {
                    // A projection is already fixed above us; this inner
                    // projection must become its own subquery source.
                    let item = self.source_from(expr)?;
                    sel.from = Some(item);
                    return Ok(());
                }
                sel.projection = Some(columns.clone());
                self.flatten_into(input, sel)
            }
            RelExpr::Filter { predicate, input } => {
                // A filter whose (transitive) input is an aggregate, and whose
                // predicate references an aggregate call, is a HAVING clause.
                if input_is_aggregate(input) && expr_has_aggregate(predicate) {
                    sel.having.push(predicate.clone());
                } else {
                    sel.wheres.push(predicate.clone());
                }
                self.flatten_into(input, sel)
            }
            RelExpr::Aggregate {
                group_by,
                aggregates,
                input,
            } => {
                if sel.grouped {
                    sel.from = Some(self.source_from(expr)?);
                    return Ok(());
                }
                sel.grouped = true;
                sel.group_by.clone_from(group_by);
                if sel.projection.is_none() {
                    sel.projection = Some(aggregate_select_list(group_by, aggregates));
                }
                self.flatten_into(input, sel)
            }
            RelExpr::Sort { keys, input } => {
                if sel.order_by.is_empty() {
                    sel.order_by.clone_from(keys);
                }
                self.flatten_into(input, sel)
            }
            RelExpr::Limit {
                count,
                offset,
                input,
            } => {
                sel.limit = Some(*count);
                if *offset > 0 {
                    sel.offset = Some(*offset);
                }
                self.flatten_into(input, sel)
            }
            RelExpr::Distinct { input } => {
                sel.distinct = true;
                self.flatten_into(input, sel)
            }
            RelExpr::DistinctOn { on, input } => {
                sel.distinct_on = Some(on.clone());
                self.flatten_into(input, sel)
            }
            RelExpr::Window { input, .. } => {
                // The optimizer keeps a `Window` node above a `Project` whose
                // select list already carries `__window_*` marker columns
                // (lowered to OVER clauses in the projection list). The node
                // itself is pure pass-through — materializing its functions
                // here would double every window column. Recurse.
                self.flatten_into(input, sel)
            }
            RelExpr::Scan { table, alias } => {
                if table == "__dual" {
                    // Synthetic single-row source for `SELECT <const>` with no
                    // FROM: emit no FROM clause at all.
                    sel.from = None;
                    return Ok(());
                }
                sel.from = Some(FromItem::Table {
                    name: table.clone(),
                    alias: alias.clone(),
                });
                Ok(())
            }
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => self.flatten_join(*join_type, condition, left, right, sel),
            // Any other relational shape (set-op, CTE, Values, table function,
            // unnest, …) cannot be spliced into the current SELECT: wrap it as
            // a derived-table subquery source.
            _ => {
                let item = self.source_from(expr)?;
                sel.from = Some(item);
                Ok(())
            }
        }
    }

    /// Flatten a join at the FROM position into `sel`. Semi/anti joins lower to
    /// a correlated `[NOT] EXISTS` predicate; other joins build a flat FROM,
    /// with any lifted arm filters bubbling up to the top-level WHERE.
    fn flatten_join(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        sel: &mut Select,
    ) -> Result<(), TranslationError> {
        // A semi-join must not multiply left rows; an anti-join keeps only
        // unmatched left rows. Neither is a plain join keyword: render the
        // left source plus `WHERE [NOT] EXISTS (SELECT 1 FROM <right> WHERE
        // <cond>)`.
        if matches!(join_type, JoinType::Semi | JoinType::Anti) {
            let exists = self.emit_semi_anti_exists(join_type, condition, right)?;
            sel.raw_wheres.push(exists);
            return self.flatten_into(left, sel);
        }
        let mut lifted: Vec<Expr> = Vec::new();
        sel.from = Some(self.build_join(join_type, condition, left, right, &mut lifted)?);
        // Lifted arm filters bubble to the top-level WHERE (not a nested ON)
        // so they see every table in scope.
        sel.wheres.extend(lifted);
        Ok(())
    }

    /// Build a flat join tree `FromItem`. Each arm becomes a table source,
    /// another (nested) join, or a derived-table subquery. Filters found over a
    /// flat arm are lifted into `lifted` (which the caller drops into the
    /// top-level WHERE) so their referenced tables are always in scope; the
    /// join's own condition stays on the `ON` (it only references the two
    /// sides being joined).
    fn build_join(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        left: &RelExpr,
        right: &RelExpr,
        lifted: &mut Vec<Expr>,
    ) -> Result<FromItem, TranslationError> {
        // An inner/cross join tree flattens into a comma-separated cross
        // product with *all* conditions (join conditions and lifted arm
        // filters) pushed to the top-level WHERE. This keeps every base-table
        // alias in a single flat scope, so conditions that reference tables
        // across (optimizer-nested) join levels still resolve — which a nested
        // `a JOIN (b JOIN c ON …a…)` tree cannot express.
        if matches!(join_type, JoinType::Inner | JoinType::Cross) {
            let mut tables: Vec<FromItem> = Vec::new();
            self.collect_inner(left, &mut tables, lifted)?;
            self.collect_inner(right, &mut tables, lifted)?;
            if matches!(join_type, JoinType::Inner) {
                lifted.push(condition.clone());
            }
            return Ok(FromItem::Cross(tables));
        }

        let join_kw = match join_type {
            JoinType::LeftOuter => "LEFT JOIN",
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
            // Semi/Anti joins are lowered to `[NOT] EXISTS` at the FROM
            // position; reaching here means one is nested as a join arm, a
            // shape we cannot emit faithfully as flat SQL.
            JoinType::Semi | JoinType::Anti => {
                return Err(TranslationError::UnsupportedFeature {
                    dialect: self.target,
                    feature: "nested semi/anti join".to_string(),
                });
            }
            JoinType::Inner | JoinType::Cross => unreachable!("handled above"),
        };

        // Outer-join arms keep their nesting (the ON condition is properly
        // scoped to the two sides). A filtered arm cannot lift to WHERE
        // without changing NULL-extension semantics, so it stays a subquery.
        let left_arm = self.join_arm(left, lifted, false)?;
        let right_arm = self.join_arm(right, lifted, false)?;
        Ok(FromItem::Join {
            left: Box::new(left_arm),
            join_kw,
            right: Box::new(right_arm),
            on: Some(condition.clone()),
        })
    }

    /// Collect the base-table sources of an inner/cross-join subtree into
    /// `tables`, pushing every join condition and lifted arm filter into
    /// `lifted` (destined for the top-level WHERE). Non-inner shapes below
    /// become a single nested/subquery source.
    fn collect_inner(
        &mut self,
        expr: &RelExpr,
        tables: &mut Vec<FromItem>,
        lifted: &mut Vec<Expr>,
    ) -> Result<(), TranslationError> {
        match expr {
            RelExpr::Scan { table, alias } => {
                tables.push(FromItem::Table {
                    name: table.clone(),
                    alias: alias.clone(),
                });
                Ok(())
            }
            RelExpr::Filter { predicate, input } if arm_is_flat(input) => {
                lifted.push(predicate.clone());
                self.collect_inner(input, tables, lifted)
            }
            RelExpr::Join {
                join_type: JoinType::Inner,
                condition,
                left,
                right,
            } => {
                self.collect_inner(left, tables, lifted)?;
                self.collect_inner(right, tables, lifted)?;
                lifted.push(condition.clone());
                Ok(())
            }
            RelExpr::Join {
                join_type: JoinType::Cross,
                left,
                right,
                ..
            } => {
                self.collect_inner(left, tables, lifted)?;
                self.collect_inner(right, tables, lifted)
            }
            // An outer/semi/anti join, aggregate, limit, etc. as an inner-join
            // arm: render it as a single nested source (its own scope).
            RelExpr::Join {
                join_type,
                condition,
                left,
                right,
            } => {
                let item = self.build_join(*join_type, condition, left, right, lifted)?;
                tables.push(item);
                Ok(())
            }
            other => {
                let item = self.source_from(other)?;
                tables.push(item);
                Ok(())
            }
        }
    }

    /// Render one arm of an *outer* join. A plain Scan stays a flat table so
    /// its alias is in scope for the ON condition. A nested outer join is
    /// rendered inline (SQL join syntax is left-associative, so a table in the
    /// inner join stays in scope for the outer ON). Anything else (an inner
    /// join subtree, aggregate, …) is wrapped in a subquery to keep
    /// associativity and NULL semantics correct.
    fn join_arm(
        &mut self,
        expr: &RelExpr,
        lifted: &mut Vec<Expr>,
        _allow_lift: bool,
    ) -> Result<FromItem, TranslationError> {
        match expr {
            RelExpr::Scan { table, alias } => Ok(FromItem::Table {
                name: table.clone(),
                alias: alias.clone(),
            }),
            RelExpr::Join {
                join_type: jt @ (JoinType::LeftOuter | JoinType::RightOuter | JoinType::FullOuter),
                condition,
                left,
                right,
            } => self.build_join(*jt, condition, left, right, lifted),
            other => self.source_from(other),
        }
    }

    /// Render any relation as a derived-table subquery `(SELECT …) AS alias`.
    fn source_from(&mut self, expr: &RelExpr) -> Result<FromItem, TranslationError> {
        // A named derived table keeps its real alias; anything else gets a
        // fresh `_sqN`.
        if let RelExpr::SubqueryAlias { alias, input } = expr {
            let sql = self.emit_rel_expr(input)?;
            return Ok(FromItem::Subquery {
                sql,
                alias: alias.clone(),
            });
        }
        let alias = self.fresh_alias();
        let sql = self.emit_rel_expr(expr)?;
        Ok(FromItem::Subquery { sql, alias })
    }

    fn fresh_alias(&mut self) -> String {
        let a = format!("_sq{}", self.alias_counter);
        self.alias_counter += 1;
        a
    }

    /// Render a semi/anti join as a correlated `[NOT] EXISTS` predicate:
    /// `[NOT] EXISTS (SELECT 1 FROM <right> WHERE <condition>)`. The right
    /// subtree keeps its base-table aliases (so the correlated condition can
    /// reference both sides), and the join condition is folded into the
    /// subquery's WHERE.
    fn emit_semi_anti_exists(
        &mut self,
        join_type: JoinType,
        condition: &Expr,
        right: &RelExpr,
    ) -> Result<String, TranslationError> {
        let mut inner = self.flatten(right)?;
        // Force the subquery projection to `SELECT 1` — EXISTS ignores it.
        inner.projection = Some(vec![ProjectionColumn {
            expr: Expr::Const(Const::Int(1)),
            alias: None,
        }]);
        inner.wheres.push(condition.clone());
        let inner_sql = self.render_select(&inner)?;
        let kw = if matches!(join_type, JoinType::Anti) {
            "NOT EXISTS"
        } else {
            "EXISTS"
        };
        Ok(format!("{kw} ({inner_sql})"))
    }

    /// Render a fully-collected `Select` into SQL.
    fn render_select(&mut self, sel: &Select) -> Result<String, TranslationError> {
        let mut sql = String::from("SELECT ");
        if sel.distinct {
            sql.push_str("DISTINCT ");
        }
        if let Some(on) = &sel.distinct_on {
            let parts: Result<Vec<String>, _> = on.iter().map(|e| self.emit_expr(e)).collect();
            let _ = write!(sql, "DISTINCT ON ({}) ", parts?.join(", "));
        }

        let select_list = match &sel.projection {
            Some(cols) => self.emit_projection_list(cols)?,
            None => "*".to_string(),
        };
        sql.push_str(&select_list);

        if let Some(from) = &sel.from {
            let _ = write!(sql, " FROM {}", self.render_from(from)?);
        }

        if !sel.wheres.is_empty() || !sel.raw_wheres.is_empty() {
            let mut parts: Vec<String> = sel
                .wheres
                .iter()
                .map(|p| self.emit_expr(p))
                .collect::<Result<_, _>>()?;
            parts.extend(sel.raw_wheres.iter().cloned());
            let _ = write!(sql, " WHERE {}", parts.join(" AND "));
        }

        if !sel.group_by.is_empty() {
            let parts: Result<Vec<String>, _> =
                sel.group_by.iter().map(|g| self.emit_expr(g)).collect();
            let _ = write!(sql, " GROUP BY {}", parts?.join(", "));
        }

        if !sel.having.is_empty() {
            let pred = self.and_combine(&sel.having)?;
            let _ = write!(sql, " HAVING {pred}");
        }

        if !sel.order_by.is_empty() {
            let order = self.emit_sort_keys(&sel.order_by)?;
            let _ = write!(sql, " ORDER BY {order}");
        }

        self.render_limit_offset(&mut sql, sel.limit, sel.offset);

        Ok(sql)
    }

    fn render_from(&mut self, item: &FromItem) -> Result<String, TranslationError> {
        match item {
            FromItem::Table { name, alias } => {
                let quoted = self.quote_ident(name);
                if let Some(a) = alias {
                    Ok(format!("{quoted} AS {}", self.quote_ident(a)))
                } else {
                    Ok(quoted)
                }
            }
            FromItem::Subquery { sql, alias } => {
                Ok(format!("({sql}) AS {}", self.quote_ident(alias)))
            }
            FromItem::Join {
                left,
                join_kw,
                right,
                on,
            } => {
                let l = self.render_from(left)?;
                let r = self.render_from(right)?;
                if let Some(cond) = on {
                    let c = self.emit_expr(cond)?;
                    Ok(format!("{l} {join_kw} {r} ON {c}"))
                } else {
                    Ok(format!("{l} {join_kw} {r}"))
                }
            }
            FromItem::Cross(items) => {
                let parts: Result<Vec<String>, _> =
                    items.iter().map(|it| self.render_from(it)).collect();
                Ok(parts?.join(", "))
            }
        }
    }

    fn and_combine(&mut self, preds: &[Expr]) -> Result<String, TranslationError> {
        let parts: Result<Vec<String>, _> = preds.iter().map(|p| self.emit_expr(p)).collect();
        Ok(parts?.join(" AND "))
    }

    fn render_limit_offset(&mut self, sql: &mut String, limit: Option<u64>, offset: Option<u64>) {
        let Some(count) = limit else {
            if let Some(off) = offset {
                if self.target.supports_limit() {
                    let _ = write!(sql, " OFFSET {off}");
                } else {
                    let _ = write!(sql, " OFFSET {off} ROWS");
                }
            }
            return;
        };
        if self.target.supports_limit() {
            if let Some(off) = offset {
                let _ = write!(sql, " LIMIT {count} OFFSET {off}");
            } else {
                let _ = write!(sql, " LIMIT {count}");
            }
        } else if self.target.supports_fetch() {
            if let Some(off) = offset {
                let _ = write!(sql, " OFFSET {off} ROWS");
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
        // Collect a chain of directly-nested CTEs into a single WITH list:
        // `WITH a AS (…), b AS (…) <body>` (nested `WITH … WITH …` is invalid).
        let mut parts = vec![format!(
            "{} AS ({})",
            self.quote_ident(name),
            self.emit_rel_expr(definition)?
        )];
        let mut cur = body;
        while let RelExpr::CTE {
            name,
            definition,
            body,
        } = cur
        {
            parts.push(format!(
                "{} AS ({})",
                self.quote_ident(name),
                self.emit_rel_expr(definition)?
            ));
            cur = body;
        }
        let body_sql = self.emit_rel_expr(cur)?;
        Ok(format!("WITH {} {body_sql}", parts.join(", ")))
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

        // Window-function markers (`__window_ROW_NUMBER`, `__window_SUM`, …)
        // carry PARTITION BY / ORDER BY as sentinel args; lower to a real
        // OVER clause.
        if let Some(stripped) = upper_name.strip_prefix("__WINDOW_") {
            return self.emit_window_marker(stripped, args);
        }

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
            // ILIKE operator — ra-parser emits __ilike(a, b) which uppercases to __ILIKE
            "__ILIKE" if args.len() == 2 => {
                let result = self.emit_ilike(&args[0], &args[1])?;
                Ok(Some(result))
            }
            // LIKE operator — ra-parser emits __like(a, b) which uppercases to __LIKE
            "__LIKE" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} LIKE {r})")))
            }
            // IN (value list): ra-parser emits __in_list(test, v1, v2, …).
            "__IN_LIST" if args.len() >= 2 => {
                let target = self.emit_expr(&args[0])?;
                let vals: Result<Vec<String>, _> =
                    args[1..].iter().map(|a| self.emit_expr(a)).collect();
                Ok(Some(format!("{target} IN ({})", vals?.join(", "))))
            }
            "__NOT_IN_LIST" if args.len() >= 2 => {
                let target = self.emit_expr(&args[0])?;
                let vals: Result<Vec<String>, _> =
                    args[1..].iter().map(|a| self.emit_expr(a)).collect();
                Ok(Some(format!("{target} NOT IN ({})", vals?.join(", "))))
            }
            // IS NULL / IS NOT NULL markers.
            "__IS_NULL" if args.len() == 1 => {
                let e = self.emit_expr(&args[0])?;
                Ok(Some(format!("({e}) IS NULL")))
            }
            "__IS_NOT_NULL" if args.len() == 1 => {
                let e = self.emit_expr(&args[0])?;
                Ok(Some(format!("({e}) IS NOT NULL")))
            }
            // JSON field access operators.
            "__JSON_FIELD" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} -> {r})")))
            }
            "__JSON_FIELD_TEXT" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} ->> {r})")))
            }
            "__JSONB_CONTAINS" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} @> {r})")))
            }
            "__JSONB_CONTAINED_BY" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} <@ {r})")))
            }
            // CASE re-encoded by the e-graph roundtrip as
            // __CASE(operand_or_null, w1, t1, …, wN, tN, else_or_null).
            "__CASE" if args.len() >= 2 && args.len().is_multiple_of(2) => {
                let result = self.emit_case_marker(args)?;
                Ok(Some(result))
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
            // SIMILAR TO operator
            "__SIMILAR_TO" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} SIMILAR TO {r})")))
            }
            // Regex match operator (~)
            "__REGEX_MATCH" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} ~ {r})")))
            }
            _ => self.emit_marker_function(name, args),
        }
    }

    /// Lower the remaining `__`-prefixed markers (JSON ops, DISTINCT, EXTRACT).
    /// Split out of [`Self::emit_special_function`] to keep each match small.
    fn emit_marker_function(
        &mut self,
        name: &str,
        args: &[Expr],
    ) -> Result<Option<String>, TranslationError> {
        match name {
            // JSON field access operators.
            "__JSON_FIELD" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} -> {r})")))
            }
            "__JSON_FIELD_TEXT" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} ->> {r})")))
            }
            "__JSONB_CONTAINS" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} @> {r})")))
            }
            "__JSONB_CONTAINED_BY" if args.len() == 2 => {
                let l = self.emit_expr(&args[0])?;
                let r = self.emit_expr(&args[1])?;
                Ok(Some(format!("({l} <@ {r})")))
            }
            // COUNT(DISTINCT x) etc. — parser encodes the DISTINCT argument as
            // __distinct(x) inside the aggregate call.
            "__DISTINCT" if args.len() == 1 => {
                let inner = self.emit_expr(&args[0])?;
                Ok(Some(format!("DISTINCT {inner}")))
            }
            // EXTRACT(field FROM expr): the parser encodes it as
            // __extract_<field>(expr) (field lower-cased; upper-cased here).
            // Lower it back to a real EXTRACT(field FROM expr).
            _ if name.len() > "__EXTRACT_".len()
                && name.starts_with("__EXTRACT_")
                && args.len() == 1 =>
            {
                let field = &name["__EXTRACT_".len()..];
                let inner = self.emit_expr(&args[0])?;
                Ok(Some(format!("EXTRACT({field} FROM {inner})")))
            }
            // Legacy encoding without the field (older parses) — cannot
            // faithfully reconstruct, so surface a clean unsupported error.
            "__EXTRACT" => Err(TranslationError::UnsupportedFeature {
                dialect: self.target,
                feature: "EXTRACT (field lost during parsing)".to_string(),
            }),
            // GROUP BY ROLLUP(...) / CUBE(...): the parser marks these as
            // __rollup(cols...) / __cube(cols...). Lower back to real SQL.
            "__ROLLUP" | "__CUBE" => {
                let kw = if name == "__ROLLUP" { "ROLLUP" } else { "CUBE" };
                let cols: Vec<String> = args
                    .iter()
                    .map(|a| self.emit_expr(a))
                    .collect::<Result<_, _>>()?;
                Ok(Some(format!("{kw} ({})", cols.join(", "))))
            }
            // GROUP BY GROUPING SETS ((a,b),(a),()): the parser marks it as
            // __grouping_sets(__gs_item(a,b), __gs_item(a), __gs_item()).
            "__GROUPING_SETS" => {
                let mut sets = Vec::with_capacity(args.len());
                for a in args {
                    let inner = self.emit_grouping_set_item(a)?;
                    sets.push(inner);
                }
                Ok(Some(format!("GROUPING SETS ({})", sets.join(", "))))
            }
            // A bare __gs_item outside GROUPING SETS: render its columns as a
            // parenthesized set (defensive; normally consumed by the arm above).
            "__GS_ITEM" => Ok(Some(self.emit_grouping_set_item_cols(args)?)),
            _ => Ok(None),
        }
    }

    /// Render one grouping-set item: `__gs_item(a, b)` -> `(a, b)`, `()`.
    fn emit_grouping_set_item(&mut self, expr: &Expr) -> Result<String, TranslationError> {
        match expr {
            Expr::Function { name, args } if name.eq_ignore_ascii_case("__gs_item") => {
                self.emit_grouping_set_item_cols(args)
            }
            // A single bare column set (not wrapped) — parenthesize it.
            other => Ok(format!("({})", self.emit_expr(other)?)),
        }
    }

    fn emit_grouping_set_item_cols(&mut self, args: &[Expr]) -> Result<String, TranslationError> {
        let cols: Vec<String> = args
            .iter()
            .map(|a| self.emit_expr(a))
            .collect::<Result<_, _>>()?;
        Ok(format!("({})", cols.join(", ")))
    }

    /// Lower a `__window_<FUNC>` marker (with `__window_partition` /
    /// `__window_order_asc|desc` sentinel args) into a real
    /// `FUNC(args) OVER (PARTITION BY … ORDER BY …)` expression.
    fn emit_window_marker(
        &mut self,
        func: &str,
        args: &[Expr],
    ) -> Result<String, TranslationError> {
        let mut real_args: Vec<String> = Vec::new();
        let mut partition: Vec<String> = Vec::new();
        let mut order: Vec<String> = Vec::new();
        let mut has_frame = false;
        for a in args {
            match a {
                Expr::Function { name, args: inner } if name == "__window_partition" => {
                    for p in inner {
                        partition.push(self.emit_expr(p)?);
                    }
                }
                Expr::Function { name, args: inner } if name == "__window_order_asc" => {
                    if let Some(e) = inner.first() {
                        order.push(format!("{} ASC", self.emit_expr(e)?));
                    }
                }
                Expr::Function { name, args: inner } if name == "__window_order_desc" => {
                    if let Some(e) = inner.first() {
                        order.push(format!("{} DESC", self.emit_expr(e)?));
                    }
                }
                Expr::Function { name, .. } if name == "__window_frame" => {
                    has_frame = true;
                }
                other => real_args.push(self.emit_expr(other)?),
            }
        }
        // A frame was specified but its bounds were dropped during parsing;
        // we cannot faithfully reproduce it, so refuse rather than emit
        // silently-wrong SQL.
        if has_frame {
            return Err(TranslationError::UnsupportedFeature {
                dialect: self.target,
                feature: "window frame (bounds lost during parsing)".to_string(),
            });
        }
        let arg_list = real_args.join(", ");
        let mut over_parts: Vec<String> = Vec::new();
        if !partition.is_empty() {
            over_parts.push(format!("PARTITION BY {}", partition.join(", ")));
        }
        if !order.is_empty() {
            over_parts.push(format!("ORDER BY {}", order.join(", ")));
        }
        Ok(format!(
            "{func}({arg_list}) OVER ({})",
            over_parts.join(" ")
        ))
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

    /// Lower a `__CASE` marker (produced by the e-graph roundtrip) into a real
    /// CASE expression. Layout:
    /// `[operand_or_null, w1, t1, …, wN, tN, else_or_null]` where a NULL const
    /// is the sentinel for "no operand" / "no ELSE".
    fn emit_case_marker(&mut self, args: &[Expr]) -> Result<String, TranslationError> {
        let n = args.len();
        let mut sql = String::from("CASE");
        if !matches!(&args[0], Expr::Const(Const::Null)) {
            let op = self.emit_expr(&args[0])?;
            let _ = write!(sql, " {op}");
        }
        let mut k = 1;
        while k < n - 1 {
            let c = self.emit_expr(&args[k])?;
            let r = self.emit_expr(&args[k + 1])?;
            let _ = write!(sql, " WHEN {c} THEN {r}");
            k += 2;
        }
        if !matches!(&args[n - 1], Expr::Const(Const::Null)) {
            let e = self.emit_expr(&args[n - 1])?;
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

    // ---- Helper: identifier quoting ----

    fn quote_ident(&self, ident: &str) -> String {
        if ident == "*" {
            return "*".to_string();
        }
        let q = self.target.quote_style();
        if q == '`' {
            format!("`{}`", ident.replace('`', "``"))
        } else {
            format!("\"{ident}\"")
        }
    }
}

/// A single flattened SELECT statement collected from a `RelExpr` chain.
#[derive(Default)]
struct Select {
    distinct: bool,
    distinct_on: Option<Vec<Expr>>,
    /// `None` means `SELECT *`.
    projection: Option<Vec<ProjectionColumn>>,
    from: Option<FromItem>,
    wheres: Vec<Expr>,
    /// Already-rendered WHERE predicates (e.g. `[NOT] EXISTS (…)` from a
    /// semi/anti join). Kept separate because they are raw SQL, not `Expr`.
    raw_wheres: Vec<String>,
    grouped: bool,
    group_by: Vec<Expr>,
    having: Vec<Expr>,
    order_by: Vec<SortKey>,
    limit: Option<u64>,
    offset: Option<u64>,
}

/// A FROM-clause item: a base table, a derived subquery, or a (flat) join.
enum FromItem {
    Table {
        name: String,
        alias: Option<String>,
    },
    Subquery {
        sql: String,
        alias: String,
    },
    Join {
        left: Box<FromItem>,
        join_kw: &'static str,
        right: Box<FromItem>,
        on: Option<Expr>,
    },
    /// Comma-separated cross product of items (a flattened inner/cross-join
    /// tree); all its join conditions live in the top-level WHERE.
    Cross(Vec<FromItem>),
}

/// Whether a relation, ignoring intervening Project/Filter/Sort/Limit, is an
/// aggregate — used to route a filter's predicate to HAVING vs WHERE.
fn input_is_aggregate(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Aggregate { .. } => true,
        RelExpr::Project { input, .. }
        | RelExpr::Sort { input, .. }
        | RelExpr::Limit { input, .. }
        | RelExpr::Distinct { input }
        | RelExpr::Window { input, .. } => input_is_aggregate(input),
        _ => false,
    }
}

/// Whether an expression references an aggregate function call (COUNT/SUM/…),
/// signalling a HAVING predicate.
fn expr_has_aggregate(expr: &Expr) -> bool {
    const AGGS: [&str; 9] = [
        "COUNT",
        "SUM",
        "AVG",
        "MIN",
        "MAX",
        "STDDEV",
        "VARIANCE",
        "STRING_AGG",
        "ARRAY_AGG",
    ];
    match expr {
        Expr::Function { name, args } => {
            AGGS.contains(&name.to_uppercase().as_str()) || args.iter().any(expr_has_aggregate)
        }
        Expr::BinOp { left, right, .. } => expr_has_aggregate(left) || expr_has_aggregate(right),
        Expr::UnaryOp { operand, .. } => expr_has_aggregate(operand),
        Expr::Cast { expr, .. } => expr_has_aggregate(expr),
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            operand.as_deref().is_some_and(expr_has_aggregate)
                || when_clauses
                    .iter()
                    .any(|(c, r)| expr_has_aggregate(c) || expr_has_aggregate(r))
                || else_result.as_deref().is_some_and(expr_has_aggregate)
        }
        _ => false,
    }
}

/// A relation is "flat" as a join arm when its FROM clause can be built from
/// base tables and joins alone (Scan / Filter / Join), so its scans stay in
/// scope. Aggregates, limits, distinct, set-ops, etc. require a subquery.
fn arm_is_flat(expr: &RelExpr) -> bool {
    match expr {
        RelExpr::Scan { .. } => true,
        RelExpr::Filter { input, .. } => arm_is_flat(input),
        RelExpr::Join {
            join_type,
            left,
            right,
            ..
        } => {
            // Semi/anti joins are lowered to EXISTS at the FROM position, not
            // as a join arm, so a nested one forces a subquery.
            !matches!(join_type, JoinType::Semi | JoinType::Anti)
                && arm_is_flat(left)
                && arm_is_flat(right)
        }
        _ => false,
    }
}

/// Build an aggregate's SELECT list from its grouping keys followed by its
/// aggregate expressions (falling back to `*` when both are empty).
/// Expand a `group_by` item into the columns that appear in the `SELECT` list.
/// Plain expressions map to themselves; `ROLLUP`/`CUBE`/`GROUPING SETS` markers
/// expand to their underlying grouping columns (deduplication is left to PG).
fn grouping_select_columns(g: &Expr) -> Vec<Expr> {
    match g {
        Expr::Function { name, args }
            if matches!(
                name.to_uppercase().as_str(),
                "__ROLLUP" | "__CUBE" | "__GROUPING_SETS" | "__GS_ITEM"
            ) =>
        {
            args.iter().flat_map(grouping_select_columns).collect()
        }
        other => vec![other.clone()],
    }
}

fn aggregate_select_list(group_by: &[Expr], aggregates: &[AggregateExpr]) -> Vec<ProjectionColumn> {
    // ROLLUP/CUBE/GROUPING SETS markers appear in the group_by list; the SELECT
    // list must show their underlying grouping columns, not the marker call.
    let mut cols: Vec<ProjectionColumn> = group_by
        .iter()
        .flat_map(grouping_select_columns)
        .map(|expr| ProjectionColumn { expr, alias: None })
        .collect();
    for agg in aggregates {
        cols.push(ProjectionColumn {
            expr: aggregate_to_expr(agg),
            alias: agg.alias.clone(),
        });
    }
    if cols.is_empty() {
        cols.push(ProjectionColumn {
            expr: Expr::Column(ra_core::expr::ColumnRef::new("*")),
            alias: None,
        });
    }
    cols
}

/// Convert an `AggregateExpr` back into a scalar `Expr::Function` for use in a
/// projection list (so a `SELECT COUNT(*)` reconstructs from an aggregate).
fn aggregate_to_expr(agg: &AggregateExpr) -> Expr {
    let name = match agg.function {
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
    let arg = agg
        .arg
        .clone()
        .unwrap_or_else(|| Expr::Column(ra_core::expr::ColumnRef::new("*")));
    let arg = if agg.distinct {
        Expr::Function {
            name: "__distinct".to_string(),
            args: vec![arg],
        }
    } else {
        arg
    };
    Expr::Function {
        name: name.to_string(),
        args: vec![arg],
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
    fn emit_rollup_and_grouping_sets_lower_to_sql() {
        use ra_core::algebra::{AggregateExpr, AggregateFunction};
        // GROUP BY ROLLUP(a, b): parser marks group_by as __rollup(a, b).
        let rollup = Expr::Function {
            name: "__rollup".to_string(),
            args: vec![
                Expr::Column(ColumnRef::new("a")),
                Expr::Column(ColumnRef::new("b")),
            ],
        };
        let agg = RelExpr::Aggregate {
            group_by: vec![rollup],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("c".to_string()),
            }],
            input: Box::new(simple_scan()),
        };
        let sql = emit_sql(&agg, Dialect::PostgreSql).expect("should emit");
        assert!(
            sql.sql
                .to_uppercase()
                .contains("GROUP BY ROLLUP (\"A\", \"B\")")
                || sql.sql.to_uppercase().contains("ROLLUP (\"A\", \"B\")"),
            "expected GROUP BY ROLLUP (a, b), got: {}",
            sql.sql
        );
        assert!(
            !sql.sql.contains("__rollup"),
            "the __rollup marker must not leak into SQL: {}",
            sql.sql
        );
    }

    #[test]
    fn emit_subquery_alias_uses_real_alias() {
        // SubqueryAlias { alias, input } emits `(<input>) AS alias` so outer
        // references resolve (Codeberg #23).
        let expr = RelExpr::SubqueryAlias {
            alias: "top_orders".to_string(),
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(
            result.sql.contains("top_orders"),
            "expected the real derived-table alias in: {}",
            result.sql
        );
        assert!(
            !result.sql.contains("_sq"),
            "should use the real alias, not a synthetic _sqN: {}",
            result.sql
        );
    }

    #[test]
    fn emit_extract_field_roundtrip() {
        // EXTRACT(YEAR FROM d) parses to __extract_year(d); the emitter must
        // lower it back to EXTRACT(YEAR FROM d), not error out (Codeberg #22).
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Function {
                    name: "__extract_year".to_string(),
                    args: vec![Expr::Column(ColumnRef::new("d"))],
                },
                alias: None,
            }],
            input: Box::new(simple_scan()),
        };
        let result = emit_sql(&expr, Dialect::PostgreSql).expect("should emit");
        assert!(
            result.sql.to_uppercase().contains("EXTRACT(YEAR FROM"),
            "expected EXTRACT(YEAR FROM ...) in: {}",
            result.sql
        );
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

    // ---- Flattened-form structural tests (RA-STEERING #22, Gate 1) ----

    /// A 2-table inner join with qualified column refs flattens into one
    /// SELECT keeping both original aliases; the join sources are not buried
    /// under a `SELECT * … AS _t` derived table.
    #[test]
    fn flatten_two_table_join_keeps_aliases() {
        let expr = RelExpr::Project {
            columns: vec![
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::qualified("o", "o_id")),
                    alias: None,
                },
                ProjectionColumn {
                    expr: Expr::Column(ColumnRef::qualified("c", "c_name")),
                    alias: None,
                },
            ],
            input: Box::new(RelExpr::Join {
                join_type: JoinType::Inner,
                condition: Expr::BinOp {
                    op: BinOp::Eq,
                    left: Box::new(Expr::Column(ColumnRef::qualified("o", "c_id"))),
                    right: Box::new(Expr::Column(ColumnRef::qualified("c", "c_id"))),
                },
                left: Box::new(RelExpr::Scan {
                    table: "orders".to_string(),
                    alias: Some("o".to_string()),
                }),
                right: Box::new(RelExpr::Scan {
                    table: "customer".to_string(),
                    alias: Some("c".to_string()),
                }),
            }),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql)
            .expect("should emit")
            .sql;
        assert!(
            sql.contains("\"orders\" AS \"o\""),
            "expected orders alias o in scope: {sql}"
        );
        assert!(
            sql.contains("\"customer\" AS \"c\""),
            "expected customer alias c in scope: {sql}"
        );
        // The base tables must be flat, not nested under a `SELECT * … AS _t`.
        assert!(
            !sql.contains("AS _t"),
            "base tables should not be buried in a derived table: {sql}"
        );
        assert!(
            sql.contains("\"o\".\"o_id\"") && sql.contains("\"c\".\"c_name\""),
            "qualified column refs should be present: {sql}"
        );
    }

    /// An aggregate with a grouping key flattens so the group key appears in
    /// the output list and a top-level GROUP BY is emitted.
    #[test]
    fn flatten_aggregate_group_by() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::qualified("o", "o_custkey"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("n".to_string()),
            }],
            input: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: Some("o".to_string()),
            }),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql)
            .expect("should emit")
            .sql;
        assert!(
            sql.contains("GROUP BY \"o\".\"o_custkey\""),
            "expected top-level GROUP BY on group key: {sql}"
        );
        // Group key present in the select list.
        assert!(
            sql.starts_with("SELECT \"o\".\"o_custkey\""),
            "expected group key in the output list: {sql}"
        );
        assert!(
            sql.contains("COUNT(*)"),
            "expected the aggregate in the output list: {sql}"
        );
        assert!(
            sql.contains("\"orders\" AS \"o\"") && !sql.contains("AS _t"),
            "scan should be flat: {sql}"
        );
    }

    /// A filter over a join emits the predicate as a top-level WHERE (not a
    /// nested derived table), keeping both aliases in scope.
    #[test]
    fn flatten_filter_over_join_where_at_top() {
        let join = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("o", "c_id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("c", "c_id"))),
            },
            left: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: Some("o".to_string()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "customer".to_string(),
                alias: Some("c".to_string()),
            }),
        };
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::qualified("o", "amount"))),
                right: Box::new(Expr::Const(Const::Int(100))),
            },
            input: Box::new(join),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql)
            .expect("should emit")
            .sql;
        assert!(
            sql.contains(" WHERE (\"o\".\"amount\" > 100)"),
            "expected top-level WHERE: {sql}"
        );
        assert!(
            sql.contains("\"orders\" AS \"o\"") && sql.contains("\"customer\" AS \"c\""),
            "both aliases should be in scope: {sql}"
        );
    }

    /// A `SELECT <const>` over the synthetic `__dual` source emits no FROM
    /// clause (`SELECT 1`, not `SELECT 1 FROM __dual`).
    #[test]
    fn flatten_dual_no_from() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Const(Const::Int(1)),
                alias: None,
            }],
            input: Box::new(RelExpr::Scan {
                table: "__dual".to_string(),
                alias: None,
            }),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql)
            .expect("should emit")
            .sql;
        assert_eq!(sql, "SELECT 1", "expected no FROM clause: {sql}");
        assert!(!sql.contains("__dual"), "__dual must not leak: {sql}");
        assert!(!sql.contains("FROM"), "no FROM clause expected: {sql}");
    }

    /// A semi join lowers to `WHERE EXISTS (…)` and an anti join to
    /// `WHERE NOT EXISTS (…)`, not a plain JOIN keyword.
    #[test]
    fn flatten_semi_anti_to_exists() {
        let make = |jt: JoinType| RelExpr::Join {
            join_type: jt,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("o", "c_id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("c", "c_id"))),
            },
            left: Box::new(RelExpr::Scan {
                table: "customer".to_string(),
                alias: Some("c".to_string()),
            }),
            right: Box::new(RelExpr::Scan {
                table: "orders".to_string(),
                alias: Some("o".to_string()),
            }),
        };
        let semi = emit_sql(&make(JoinType::Semi), Dialect::PostgreSql)
            .expect("emit")
            .sql;
        assert!(
            semi.contains("WHERE EXISTS (") && !semi.contains("NOT EXISTS"),
            "semi join should be WHERE EXISTS: {semi}"
        );
        let anti = emit_sql(&make(JoinType::Anti), Dialect::PostgreSql)
            .expect("emit")
            .sql;
        assert!(
            anti.contains("WHERE NOT EXISTS ("),
            "anti join should be WHERE NOT EXISTS: {anti}"
        );
    }

    /// A `__window_*` marker in a projection lowers to a real OVER clause.
    #[test]
    fn flatten_window_marker_to_over() {
        let expr = RelExpr::Project {
            columns: vec![ProjectionColumn {
                expr: Expr::Function {
                    name: "__window_ROW_NUMBER".to_string(),
                    args: vec![
                        Expr::Function {
                            name: "__window_partition".to_string(),
                            args: vec![Expr::Column(ColumnRef::new("dept"))],
                        },
                        Expr::Function {
                            name: "__window_order_desc".to_string(),
                            args: vec![Expr::Column(ColumnRef::new("salary"))],
                        },
                    ],
                },
                alias: Some("rn".to_string()),
            }],
            input: Box::new(RelExpr::Scan {
                table: "emp".to_string(),
                alias: None,
            }),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql).expect("emit").sql;
        assert!(
            sql.contains("ROW_NUMBER() OVER (PARTITION BY \"dept\" ORDER BY \"salary\" DESC)"),
            "expected lowered OVER clause: {sql}"
        );
        assert!(
            !sql.contains("__window"),
            "window marker must not leak: {sql}"
        );
    }

    /// A `HAVING` (Filter over Aggregate referencing an aggregate) emits a
    /// real HAVING clause, not a WHERE.
    #[test]
    fn flatten_having_clause() {
        let agg = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("dept"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("n".to_string()),
            }],
            input: Box::new(RelExpr::Scan {
                table: "emp".to_string(),
                alias: None,
            }),
        };
        let expr = RelExpr::Filter {
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Function {
                    name: "COUNT".to_string(),
                    args: vec![Expr::Column(ColumnRef::new("*"))],
                }),
                right: Box::new(Expr::Const(Const::Int(5))),
            },
            input: Box::new(agg),
        };
        let sql = emit_sql(&expr, Dialect::PostgreSql).expect("emit").sql;
        assert!(sql.contains("HAVING"), "expected HAVING: {sql}");
        assert!(
            !sql.contains("WHERE (COUNT"),
            "aggregate predicate must not be a WHERE: {sql}"
        );
    }
}
