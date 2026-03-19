//! SQL rendering from relational algebra expressions.
//!
//! Converts a [`RelExpr`] tree into a SQL string. Produces standard
//! SQL that is readable and suitable for display to users.

use ra_core::{
    AggregateExpr, ColumnRef, Const, Expr, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey, UnaryOp,
};

/// Renders [`RelExpr`] trees to SQL strings.
pub struct SqlRenderer;

impl SqlRenderer {
    /// Render a relational expression to a SQL string.
    #[must_use]
    pub fn render(expr: &RelExpr) -> String {
        let mut ctx = RenderContext::default();
        render_expr(expr, &mut ctx);
        ctx.finish()
    }
}

#[derive(Default)]
struct RenderContext {
    select: String,
    from: String,
    joins: Vec<String>,
    where_clauses: Vec<String>,
    group_by: Vec<String>,
    order_by: Vec<String>,
    limit: Option<u64>,
    offset: Option<u64>,
}

impl RenderContext {
    fn finish(self) -> String {
        let mut parts = Vec::new();

        let select = if self.select.is_empty() {
            "SELECT *".to_string()
        } else {
            format!("SELECT {}", self.select)
        };
        parts.push(select);

        if !self.from.is_empty() {
            parts.push(format!("FROM {}", self.from));
        }

        for join in &self.joins {
            parts.push(join.clone());
        }

        if !self.where_clauses.is_empty() {
            parts.push(format!(
                "WHERE {}",
                self.where_clauses.join(" AND ")
            ));
        }

        if !self.group_by.is_empty() {
            parts.push(format!(
                "GROUP BY {}",
                self.group_by.join(", ")
            ));
        }

        if !self.order_by.is_empty() {
            parts.push(format!(
                "ORDER BY {}",
                self.order_by.join(", ")
            ));
        }

        if let Some(limit) = self.limit {
            parts.push(format!("LIMIT {limit}"));
        }

        if let Some(offset) = self.offset {
            if offset > 0 {
                parts.push(format!("OFFSET {offset}"));
            }
        }

        parts.join("\n")
    }
}

fn render_expr(expr: &RelExpr, ctx: &mut RenderContext) {
    match expr {
        RelExpr::Scan { table, alias } => {
            ctx.from = match alias {
                Some(a) => format!("{table} AS {a}"),
                None => table.clone(),
            };
        }
        RelExpr::Filter {
            predicate, input, ..
        } => {
            render_expr(input, ctx);
            ctx.where_clauses.push(render_scalar(predicate));
        }
        RelExpr::Project {
            columns, input, ..
        } => {
            render_expr(input, ctx);
            ctx.select = render_projection(columns);
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            render_expr(left, ctx);
            let right_table = extract_table_name(right);
            let join_kw = render_join_type(*join_type);
            let cond = render_scalar(condition);
            ctx.joins.push(format!(
                "{join_kw} JOIN {right_table} ON {cond}"
            ));
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            render_expr(input, ctx);
            ctx.select = render_aggregate_select(aggregates, group_by);
            ctx.group_by = group_by
                .iter()
                .map(render_scalar)
                .collect();
        }
        RelExpr::Sort { keys, input } => {
            render_expr(input, ctx);
            ctx.order_by =
                keys.iter().map(render_sort_key).collect();
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            render_expr(input, ctx);
            ctx.limit = Some(*count);
            ctx.offset = Some(*offset);
        }
        RelExpr::Union { all, left, right } => {
            let l = SqlRenderer::render(left);
            let r = SqlRenderer::render(right);
            let kw = if *all { "UNION ALL" } else { "UNION" };
            ctx.from = format!("({l}) {kw} ({r})");
        }
        RelExpr::Intersect { all, left, right } => {
            let l = SqlRenderer::render(left);
            let r = SqlRenderer::render(right);
            let kw =
                if *all { "INTERSECT ALL" } else { "INTERSECT" };
            ctx.from = format!("({l}) {kw} ({r})");
        }
        RelExpr::Except { all, left, right } => {
            let l = SqlRenderer::render(left);
            let r = SqlRenderer::render(right);
            let kw = if *all { "EXCEPT ALL" } else { "EXCEPT" };
            ctx.from = format!("({l}) {kw} ({r})");
        }
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            let def_sql = SqlRenderer::render(definition);
            let body_sql = SqlRenderer::render(body);
            ctx.from = format!(
                "-- CTE: WITH {name} AS ({def_sql})\n{body_sql}"
            );
        }
        RelExpr::Window {
            functions, input, ..
        } => {
            render_expr(input, ctx);
            for wf in functions {
                let func_name = format!("{}", wf.function);
                let arg = match &wf.arg {
                    Some(a) => render_scalar(a),
                    None => String::new(),
                };
                let partition = if wf.partition_by.is_empty() {
                    String::new()
                } else {
                    let parts: Vec<String> = wf
                        .partition_by
                        .iter()
                        .map(render_scalar)
                        .collect();
                    format!("PARTITION BY {}", parts.join(", "))
                };
                let order = if wf.order_by.is_empty() {
                    String::new()
                } else {
                    let parts: Vec<String> = wf
                        .order_by
                        .iter()
                        .map(render_sort_key)
                        .collect();
                    format!("ORDER BY {}", parts.join(", "))
                };
                let over_parts: Vec<&str> = [
                    partition.as_str(),
                    order.as_str(),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                let alias = match &wf.alias {
                    Some(a) => format!(" AS {a}"),
                    None => String::new(),
                };
                if !ctx.select.is_empty() {
                    ctx.select.push_str(", ");
                }
                ctx.select.push_str(&format!(
                    "{func_name}({arg}) OVER ({}){alias}",
                    over_parts.join(" ")
                ));
            }
        }
        RelExpr::Distinct { input } => {
            render_expr(input, ctx);
            let current = if ctx.select.is_empty() {
                "*".to_string()
            } else {
                ctx.select.clone()
            };
            ctx.select = format!("DISTINCT {current}");
        }
        RelExpr::RecursiveCTE {
            name,
            base_case,
            recursive_case,
            body,
            ..
        } => {
            let base_sql = SqlRenderer::render(base_case);
            let rec_sql = SqlRenderer::render(recursive_case);
            let body_sql = SqlRenderer::render(body);
            ctx.from = format!(
                "-- WITH RECURSIVE {name} AS \
                 ({base_sql} UNION ALL {rec_sql})\n{body_sql}"
            );
        }
        RelExpr::Values { rows } => {
            let row_strs: Vec<String> = rows
                .iter()
                .map(|row| {
                    let vals: Vec<String> =
                        row.iter().map(render_scalar).collect();
                    format!("({})", vals.join(", "))
                })
                .collect();
            ctx.from =
                format!("(VALUES {}) AS t", row_strs.join(", "));
        }
        RelExpr::Unnest {
            expr, alias, input, ..
        } => {
            if let Some(inp) = input {
                render_expr(inp, ctx);
            }
            let alias_str = match alias {
                Some(a) => format!(" AS {a}"),
                None => String::new(),
            };
            ctx.from = format!(
                "unnest({}){alias_str}",
                render_scalar(expr)
            );
        }
        RelExpr::TableFunction {
            name, args, input, ..
        } => {
            if let Some(inp) = input {
                render_expr(inp, ctx);
            }
            let arg_strs: Vec<String> =
                args.iter().map(render_scalar).collect();
            ctx.from = format!(
                "{name}({})",
                arg_strs.join(", ")
            );
        }
        RelExpr::RowPattern { input, pattern, .. } => {
            render_expr(input, ctx);
            ctx.from = format!(
                "{} MATCH_RECOGNIZE(PATTERN ({pattern}))",
                ctx.from
            );
        }
    }
}

fn render_scalar(expr: &Expr) -> String {
    match expr {
        Expr::Column(col) => render_column_ref(col),
        Expr::Const(c) => render_const(c),
        Expr::BinOp { op, left, right } => {
            let l = render_scalar(left);
            let r = render_scalar(right);
            format!("{l} {op} {r}")
        }
        Expr::UnaryOp { op, operand } => {
            let operand_str = render_scalar(operand);
            match op {
                UnaryOp::Not => format!("NOT {operand_str}"),
                UnaryOp::IsNull => {
                    format!("{operand_str} IS NULL")
                }
                UnaryOp::IsNotNull => {
                    format!("{operand_str} IS NOT NULL")
                }
                UnaryOp::Neg => format!("-{operand_str}"),
            }
        }
        Expr::Function { name, args } => {
            let arg_strs: Vec<String> =
                args.iter().map(render_scalar).collect();
            format!("{name}({})", arg_strs.join(", "))
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => render_case(
            operand.as_deref(),
            when_clauses,
            else_result.as_deref(),
        ),
        Expr::Cast {
            expr, target_type, ..
        } => {
            format!("CAST({} AS {target_type})", render_scalar(expr))
        }
        Expr::Array(elements) => {
            let elems: Vec<String> =
                elements.iter().map(render_scalar).collect();
            format!("ARRAY[{}]", elems.join(", "))
        }
        Expr::ArrayIndex(array, index) => {
            format!(
                "{}[{}]",
                render_scalar(array),
                render_scalar(index)
            )
        }
        Expr::PatternPrev(inner, offset) => {
            format!("PREV({}, {offset})", render_scalar(inner))
        }
        Expr::PatternNext(inner, offset) => {
            format!("NEXT({}, {offset})", render_scalar(inner))
        }
        Expr::PatternFirst(inner, var) => {
            format!("FIRST({var}.{})", render_scalar(inner))
        }
        Expr::PatternLast(inner, var) => {
            format!("LAST({var}.{})", render_scalar(inner))
        }
        Expr::PatternClassifier => "CLASSIFIER()".to_owned(),
        Expr::PatternMatchNumber => {
            "MATCH_NUMBER()".to_owned()
        }
    }
}

fn render_case(
    operand: Option<&Expr>,
    when_clauses: &[(Expr, Expr)],
    else_result: Option<&Expr>,
) -> String {
    let mut parts = vec!["CASE".to_string()];
    if let Some(op) = operand {
        parts.push(render_scalar(op));
    }
    for (cond, result) in when_clauses {
        parts.push(format!(
            "WHEN {} THEN {}",
            render_scalar(cond),
            render_scalar(result)
        ));
    }
    if let Some(el) = else_result {
        parts.push(format!("ELSE {}", render_scalar(el)));
    }
    parts.push("END".to_string());
    parts.join(" ")
}

fn render_column_ref(col: &ColumnRef) -> String {
    col.to_string()
}

fn render_const(c: &Const) -> String {
    match c {
        Const::Null => "NULL".to_string(),
        Const::Bool(b) => {
            if *b { "TRUE" } else { "FALSE" }.to_string()
        }
        Const::Int(i) => i.to_string(),
        Const::Float(f) => format!("{f}"),
        Const::String(s) => format!("'{s}'"),
    }
}

fn render_projection(columns: &[ProjectionColumn]) -> String {
    columns
        .iter()
        .map(|pc| {
            let expr_str = render_scalar(&pc.expr);
            match &pc.alias {
                Some(a) => format!("{expr_str} AS {a}"),
                None => expr_str,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_aggregate_select(
    aggregates: &[AggregateExpr],
    group_by: &[Expr],
) -> String {
    let mut parts: Vec<String> = group_by
        .iter()
        .map(render_scalar)
        .collect();

    for agg in aggregates {
        let arg_str = match &agg.arg {
            Some(arg) => render_scalar(arg),
            None => "*".to_string(),
        };
        let distinct = if agg.distinct { "DISTINCT " } else { "" };
        let func = format!("{}({distinct}{arg_str})", agg.function);
        let with_alias = match &agg.alias {
            Some(a) => format!("{func} AS {a}"),
            None => func,
        };
        parts.push(with_alias);
    }
    parts.join(", ")
}

fn render_join_type(jt: JoinType) -> &'static str {
    match jt {
        JoinType::Inner => "INNER",
        JoinType::LeftOuter => "LEFT OUTER",
        JoinType::RightOuter => "RIGHT OUTER",
        JoinType::FullOuter => "FULL OUTER",
        JoinType::Cross => "CROSS",
        JoinType::Semi => "SEMI",
        JoinType::Anti => "ANTI",
    }
}

fn render_sort_key(key: &SortKey) -> String {
    let expr_str = render_scalar(&key.expr);
    let dir = match key.direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    };
    let nulls = match key.nulls {
        NullOrdering::First => " NULLS FIRST",
        NullOrdering::Last => " NULLS LAST",
    };
    format!("{expr_str} {dir}{nulls}")
}

fn extract_table_name(expr: &RelExpr) -> String {
    match expr {
        RelExpr::Scan { table, alias } => match alias {
            Some(a) => format!("{table} AS {a}"),
            None => table.clone(),
        },
        _ => SqlRenderer::render(expr),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::{
        AggregateFunction, BinOp, ProjectionColumn, SortDirection,
        SortKey,
    };

    #[test]
    fn render_simple_scan() {
        let expr = RelExpr::scan("users");
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("SELECT *"));
        assert!(sql.contains("FROM users"));
    }

    #[test]
    fn render_scan_with_filter() {
        let expr = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(25))),
        });
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("WHERE age > 25"));
    }

    #[test]
    fn render_projection() {
        let expr = RelExpr::scan("users").project(vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            },
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("email")),
                alias: None,
            },
        ]);
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("SELECT name, email"));
    }

    #[test]
    fn render_with_limit() {
        let expr = RelExpr::scan("users").limit(10, 0);
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("LIMIT 10"));
    }

    #[test]
    fn render_with_offset() {
        let expr = RelExpr::scan("users").limit(10, 5);
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("LIMIT 10"));
        assert!(sql.contains("OFFSET 5"));
    }

    #[test]
    fn render_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Inner,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(
                    ColumnRef::qualified("orders", "user_id"),
                )),
                right: Box::new(Expr::Column(
                    ColumnRef::qualified("users", "id"),
                )),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("INNER JOIN"));
        assert!(sql.contains("orders.user_id = users.id"));
    }

    #[test]
    fn render_aggregate() {
        let expr = RelExpr::Aggregate {
            group_by: vec![Expr::Column(ColumnRef::new("status"))],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: Some("cnt".into()),
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("COUNT(*)"));
        assert!(sql.contains("GROUP BY status"));
    }

    #[test]
    fn render_sort() {
        let expr = RelExpr::Sort {
            keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("name")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("users")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("ORDER BY name DESC NULLS LAST"));
    }

    #[test]
    fn render_const_values() {
        assert_eq!(render_const(&Const::Null), "NULL");
        assert_eq!(render_const(&Const::Bool(true)), "TRUE");
        assert_eq!(render_const(&Const::Int(42)), "42");
        assert_eq!(
            render_const(&Const::String("hello".into())),
            "'hello'"
        );
    }

    #[test]
    fn render_unary_ops() {
        let is_null = Expr::UnaryOp {
            op: UnaryOp::IsNull,
            operand: Box::new(Expr::Column(ColumnRef::new("x"))),
        };
        assert_eq!(render_scalar(&is_null), "x IS NULL");

        let not_expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::Const(Const::Bool(true))),
        };
        assert_eq!(render_scalar(&not_expr), "NOT TRUE");
    }

    #[test]
    fn render_cast() {
        let cast = Expr::Cast {
            expr: Box::new(Expr::Column(ColumnRef::new("age"))),
            target_type: "TEXT".into(),
        };
        assert_eq!(render_scalar(&cast), "CAST(age AS TEXT)");
    }
}
