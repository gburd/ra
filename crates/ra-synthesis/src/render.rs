//! SQL rendering from relational algebra expressions.
//!
//! Converts a [`RelExpr`] tree into a SQL string. Produces standard
//! SQL that is readable and suitable for display to users.

use ra_core::{
    AggregateExpr, ColumnRef, Const, Expr, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey, SubQueryType,
    UnaryOp,
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
        RelExpr::IndexScan { table, column } => {
            ctx.from = format!("-- Index Scan on {table} ({column})");
        }
        RelExpr::IndexOnlyScan { table, index, predicate, .. } => {
            ctx.from = format!("-- Index-Only Scan on {table} using {index}");
            ctx.where_clauses.push(render_scalar(predicate));
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
        RelExpr::IncrementalSort {
            prefix_keys,
            suffix_keys,
            input,
        } => {
            render_expr(input, ctx);
            ctx.order_by = prefix_keys
                .iter()
                .chain(suffix_keys.iter())
                .map(render_sort_key)
                .collect();
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
        RelExpr::MultiUnnest {
            exprs, aliases, ..
        } => {
            let arg_strs: Vec<String> =
                exprs.iter().map(render_scalar).collect();
            let alias_list: Vec<String> = aliases
                .iter()
                .filter_map(|a| a.clone())
                .collect();
            let alias_str = if alias_list.is_empty() {
                String::new()
            } else {
                format!(" AS t({})", alias_list.join(", "))
            };
            ctx.from = format!(
                "unnest({}){alias_str}",
                arg_strs.join(", ")
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
        // Bitmap scan operators (PostgreSQL-specific)
        RelExpr::BitmapIndexScan { table, index, predicate, .. } => {
            ctx.from = format!("-- Bitmap Index Scan on {table} using {index}");
            ctx.where_clauses.push(render_scalar(predicate));
        }
        RelExpr::BitmapAnd { inputs } => {
            ctx.from = format!("-- BitmapAnd({} inputs)", inputs.len());
        }
        RelExpr::BitmapOr { inputs } => {
            ctx.from = format!("-- BitmapOr({} inputs)", inputs.len());
        }
        RelExpr::BitmapHeapScan { bitmap, table, recheck_cond, .. } => {
            render_expr(bitmap, ctx);
            ctx.from = format!("-- Bitmap Heap Scan on {table}");
            if let Some(cond) = recheck_cond {
                ctx.where_clauses.push(render_scalar(cond));
            }
        }
        // Parallel query operators (PostgreSQL-specific)
        RelExpr::ParallelScan { table, workers, .. } => {
            ctx.from = format!("-- Parallel Seq Scan on {table} (workers={workers})");
        }
        RelExpr::ParallelHashJoin { left, right, condition, workers, .. } => {
            render_expr(left, ctx);
            let right_table = extract_table_name(right);
            let cond = render_scalar(condition);
            ctx.joins.push(format!(
                "-- Parallel Hash Join (workers={workers}) {right_table} ON {cond}"
            ));
        }
        RelExpr::ParallelAggregate { group_by, aggregates, input, workers } => {
            render_expr(input, ctx);
            ctx.select = render_aggregate_select(aggregates, group_by);
            ctx.group_by = group_by
                .iter()
                .map(render_scalar)
                .collect();
            ctx.from = format!("{} -- Parallel Aggregate (workers={workers})", ctx.from);
        }
        RelExpr::Gather { input, workers } => {
            render_expr(input, ctx);
            ctx.from = format!("{} -- Gather (workers={workers})", ctx.from);
        }
        RelExpr::MvScan { view_name, alias } => {
            ctx.from = match alias {
                Some(a) => format!("{view_name} AS {a} -- MV Scan"),
                None => format!("{view_name} -- MV Scan"),
            };
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
        Expr::ArraySlice {
            array, start, end,
        } => {
            let s = start
                .as_ref()
                .map_or(String::new(), |e| render_scalar(e));
            let e = end
                .as_ref()
                .map_or(String::new(), |e| render_scalar(e));
            format!("{}[{s}:{e}]", render_scalar(array))
        }
        Expr::FieldAccess { expr, field_name } => {
            format!("({}).{field_name}", render_scalar(expr))
        }
        Expr::SubQuery {
            subquery_type,
            query,
            test_expr,
        } => {
            let subquery_sql = SqlRenderer::render(query);
            match subquery_type {
                SubQueryType::Scalar => {
                    format!("({})", subquery_sql)
                }
                SubQueryType::Exists => {
                    format!("EXISTS ({})", subquery_sql)
                }
                SubQueryType::In => {
                    if let Some(test) = test_expr {
                        format!("{} IN ({})", render_scalar(test), subquery_sql)
                    } else {
                        format!("IN ({})", subquery_sql)
                    }
                }
                SubQueryType::Any => {
                    if let Some(test) = test_expr {
                        format!("{} = ANY ({})", render_scalar(test), subquery_sql)
                    } else {
                        format!("= ANY ({})", subquery_sql)
                    }
                }
                SubQueryType::All => {
                    if let Some(test) = test_expr {
                        format!("{} > ALL ({})", render_scalar(test), subquery_sql)
                    } else {
                        format!("> ALL ({})", subquery_sql)
                    }
                }
            }
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
    fn render_project() {
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

    // === Additional comprehensive tests for missing coverage ===

    #[test]
    fn render_index_scan() {
        let expr = RelExpr::IndexScan {
            table: "users".into(),
            column: "id".into(),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Index Scan on users"));
        assert!(sql.contains("id"));
    }

    #[test]
    fn render_index_only_scan() {
        let expr = RelExpr::IndexOnlyScan {
            table: "users".into(),
            index: "idx_email".into(),
            predicate: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("email"))),
                right: Box::new(Expr::Const(Const::String("test@example.com".into()))),
            },
            columns: vec![],
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Index-Only Scan"));
        assert!(sql.contains("idx_email"));
        assert!(sql.contains("WHERE"));
    }

    #[test]
    fn render_left_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::LeftOuter,
            condition: Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::qualified("users", "id"))),
                right: Box::new(Expr::Column(ColumnRef::qualified("orders", "user_id"))),
            },
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("LEFT OUTER JOIN"));
    }

    #[test]
    fn render_right_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::RightOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("RIGHT OUTER JOIN"));
    }

    #[test]
    fn render_full_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::FullOuter,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("FULL OUTER JOIN"));
    }

    #[test]
    fn render_cross_join() {
        let expr = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("products")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("CROSS JOIN"));
    }

    #[test]
    fn render_incremental_sort() {
        let expr = RelExpr::IncrementalSort {
            prefix_keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("category")),
                direction: SortDirection::Asc,
                nulls: NullOrdering::First,
            }],
            suffix_keys: vec![SortKey {
                expr: Expr::Column(ColumnRef::new("price")),
                direction: SortDirection::Desc,
                nulls: NullOrdering::Last,
            }],
            input: Box::new(RelExpr::scan("products")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("ORDER BY"));
        assert!(sql.contains("category"));
        assert!(sql.contains("price"));
    }

    #[test]
    fn render_union() {
        let expr = RelExpr::Union {
            all: false,
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("admins")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("UNION"));
        assert!(!sql.contains("UNION ALL"));
    }

    #[test]
    fn render_union_all() {
        let expr = RelExpr::Union {
            all: true,
            left: Box::new(RelExpr::scan("users")),
            right: Box::new(RelExpr::scan("admins")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("UNION ALL"));
    }

    #[test]
    fn render_intersect() {
        let expr = RelExpr::Intersect {
            all: false,
            left: Box::new(RelExpr::scan("customers")),
            right: Box::new(RelExpr::scan("subscribers")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("INTERSECT"));
        assert!(!sql.contains("INTERSECT ALL"));
    }

    #[test]
    fn render_intersect_all() {
        let expr = RelExpr::Intersect {
            all: true,
            left: Box::new(RelExpr::scan("customers")),
            right: Box::new(RelExpr::scan("subscribers")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("INTERSECT ALL"));
    }

    #[test]
    fn render_except() {
        let expr = RelExpr::Except {
            all: false,
            left: Box::new(RelExpr::scan("all_users")),
            right: Box::new(RelExpr::scan("banned_users")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("EXCEPT"));
        assert!(!sql.contains("EXCEPT ALL"));
    }

    #[test]
    fn render_except_all() {
        let expr = RelExpr::Except {
            all: true,
            left: Box::new(RelExpr::scan("all_users")),
            right: Box::new(RelExpr::scan("banned_users")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("EXCEPT ALL"));
    }

    #[test]
    fn render_cte() {
        let expr = RelExpr::CTE {
            name: "temp_data".into(),
            definition: Box::new(RelExpr::scan("users")),
            body: Box::new(RelExpr::scan("temp_data")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("CTE"));
        assert!(sql.contains("WITH"));
        assert!(sql.contains("temp_data"));
    }

    #[test]
    fn render_window_function() {
        use ra_core::{WindowExpr, WindowFunction};
        let expr = RelExpr::Window {
            functions: vec![WindowExpr {
                function: WindowFunction::RowNumber,
                arg: None,
                partition_by: vec![Expr::Column(ColumnRef::new("category"))],
                order_by: vec![SortKey {
                    expr: Expr::Column(ColumnRef::new("price")),
                    direction: SortDirection::Desc,
                    nulls: NullOrdering::Last,
                }],
                frame: None,
                alias: Some("rn".into()),
            }],
            input: Box::new(RelExpr::scan("products")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("OVER"));
        assert!(sql.contains("PARTITION BY"));
        assert!(sql.contains("ORDER BY"));
    }

    #[test]
    fn render_window_function_no_partition() {
        use ra_core::{WindowExpr, WindowFunction};
        let expr = RelExpr::Window {
            functions: vec![WindowExpr {
                function: WindowFunction::Sum,
                arg: Some(Expr::Column(ColumnRef::new("amount"))),
                partition_by: vec![],
                order_by: vec![],
                frame: None,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("orders")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("SUM"));
        assert!(sql.contains("OVER"));
    }

    #[test]
    fn render_distinct() {
        let expr = RelExpr::Distinct {
            input: Box::new(RelExpr::scan("users")),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("DISTINCT"));
    }

    #[test]
    fn render_distinct_with_projection() {
        let expr = RelExpr::Distinct {
            input: Box::new(
                RelExpr::scan("users").project(vec![ProjectionColumn {
                    expr: Expr::Column(ColumnRef::new("city")),
                    alias: None,
                }]),
            ),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("DISTINCT"));
        assert!(sql.contains("city"));
    }

    #[test]
    fn render_recursive_cte() {
        let expr = RelExpr::RecursiveCTE {
            name: "tree".into(),
            base_case: Box::new(RelExpr::scan("root")),
            recursive_case: Box::new(RelExpr::scan("children")),
            body: Box::new(RelExpr::scan("tree")),
            cycle_detection: None,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("RECURSIVE"));
        assert!(sql.contains("tree"));
    }

    #[test]
    fn render_values() {
        let expr = RelExpr::Values {
            rows: vec![
                vec![Expr::Const(Const::Int(1)), Expr::Const(Const::String("a".into()))],
                vec![Expr::Const(Const::Int(2)), Expr::Const(Const::String("b".into()))],
            ],
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("VALUES"));
        assert!(sql.contains("(1, 'a')"));
        assert!(sql.contains("(2, 'b')"));
    }

    #[test]
    fn render_unnest() {
        let expr = RelExpr::Unnest {
            expr: Expr::Array(vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(2))]),
            alias: Some("arr".into()),
            input: None,
            with_ordinality: false,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("unnest"));
        assert!(sql.contains("AS arr"));
    }

    #[test]
    fn render_unnest_without_alias() {
        let expr = RelExpr::Unnest {
            expr: Expr::Column(ColumnRef::new("items")),
            alias: None,
            input: None,
            with_ordinality: false,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("unnest"));
        assert!(!sql.contains(" AS "));
    }

    #[test]
    fn render_multi_unnest() {
        let expr = RelExpr::MultiUnnest {
            exprs: vec![
                Expr::Column(ColumnRef::new("arr1")),
                Expr::Column(ColumnRef::new("arr2")),
            ],
            aliases: vec![Some("a".into()), Some("b".into())],
            with_ordinality: false,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("unnest"));
        assert!(sql.contains("arr1"));
        assert!(sql.contains("arr2"));
    }

    #[test]
    fn render_multi_unnest_no_aliases() {
        let expr = RelExpr::MultiUnnest {
            exprs: vec![Expr::Column(ColumnRef::new("arr1"))],
            aliases: vec![None],
            with_ordinality: false,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("unnest"));
    }

    #[test]
    fn render_table_function() {
        let expr = RelExpr::TableFunction {
            name: "generate_series".into(),
            args: vec![Expr::Const(Const::Int(1)), Expr::Const(Const::Int(10))],
            columns: vec![],
            input: None,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("generate_series(1, 10)"));
    }

    #[test]
    fn render_row_pattern() {
        use ra_core::row_pattern::{MatchMode, PatternExpr, SkipMode};

        let expr = RelExpr::RowPattern {
            input: Box::new(RelExpr::scan("events")),
            partition_by: vec![],
            order_by: vec![],
            pattern: PatternExpr::Var("A".into()),
            defines: vec![],
            measures: vec![],
            mode: MatchMode::OneRowPerMatch,
            skip_mode: SkipMode::PastLastRow,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("MATCH_RECOGNIZE"));
        assert!(sql.contains("PATTERN"));
    }

    #[test]
    fn render_bitmap_index_scan() {
        let expr = RelExpr::BitmapIndexScan {
            table: "users".into(),
            index: "idx_age".into(),
            predicate: Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(30))),
            },
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Bitmap Index Scan"));
        assert!(sql.contains("idx_age"));
    }

    #[test]
    fn render_bitmap_and() {
        let expr = RelExpr::BitmapAnd {
            inputs: vec![
                Box::new(RelExpr::scan("t1")),
                Box::new(RelExpr::scan("t2")),
            ],
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("BitmapAnd"));
    }

    #[test]
    fn render_bitmap_or() {
        let expr = RelExpr::BitmapOr {
            inputs: vec![
                Box::new(RelExpr::scan("t1")),
                Box::new(RelExpr::scan("t2")),
                Box::new(RelExpr::scan("t3")),
            ],
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("BitmapOr"));
        assert!(sql.contains("3 inputs"));
    }

    #[test]
    fn render_bitmap_heap_scan() {
        let expr = RelExpr::BitmapHeapScan {
            bitmap: Box::new(RelExpr::scan("idx")),
            table: "users".into(),
            recheck_cond: Some(Expr::Const(Const::Bool(true))),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Bitmap Heap Scan"));
    }

    #[test]
    fn render_bitmap_heap_scan_no_recheck() {
        let expr = RelExpr::BitmapHeapScan {
            bitmap: Box::new(RelExpr::scan("idx")),
            table: "users".into(),
            recheck_cond: None,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Bitmap Heap Scan"));
        assert!(sql.contains("users"));
    }

    #[test]
    fn render_parallel_scan() {
        let expr = RelExpr::ParallelScan {
            table: "large_table".into(),
            workers: 4,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Parallel Seq Scan"));
        assert!(sql.contains("workers=4"));
    }

    #[test]
    fn render_parallel_hash_join() {
        let expr = RelExpr::ParallelHashJoin {
            join_type: JoinType::Inner,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(RelExpr::scan("orders")),
            right: Box::new(RelExpr::scan("customers")),
            workers: 8,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Parallel Hash Join"));
        assert!(sql.contains("workers=8"));
    }

    #[test]
    fn render_parallel_aggregate() {
        let expr = RelExpr::ParallelAggregate {
            group_by: vec![],
            aggregates: vec![AggregateExpr {
                function: AggregateFunction::Count,
                arg: None,
                distinct: false,
                alias: None,
            }],
            input: Box::new(RelExpr::scan("events")),
            workers: 4,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Parallel Aggregate"));
        assert!(sql.contains("workers=4"));
    }

    #[test]
    fn render_gather() {
        let expr = RelExpr::Gather {
            input: Box::new(RelExpr::scan("data")),
            workers: 2,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("Gather"));
        assert!(sql.contains("workers=2"));
    }

    #[test]
    fn render_mv_scan() {
        let expr = RelExpr::MvScan {
            view_name: "mat_view".into(),
            alias: None,
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("mat_view"));
        assert!(sql.contains("MV Scan"));
    }

    #[test]
    fn render_mv_scan_with_alias() {
        let expr = RelExpr::MvScan {
            view_name: "mat_view".into(),
            alias: Some("mv".into()),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("AS mv"));
    }

    // === Expression rendering tests ===

    #[test]
    fn render_is_not_null() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(Expr::Column(ColumnRef::new("email"))),
        };
        assert_eq!(render_scalar(&expr), "email IS NOT NULL");
    }

    #[test]
    fn render_negation() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Const(Const::Int(42))),
        };
        assert_eq!(render_scalar(&expr), "-42");
    }

    #[test]
    fn render_function() {
        let expr = Expr::Function {
            name: "UPPER".into(),
            args: vec![Expr::Column(ColumnRef::new("name"))],
        };
        assert_eq!(render_scalar(&expr), "UPPER(name)");
    }

    #[test]
    fn render_function_multiple_args() {
        let expr = Expr::Function {
            name: "SUBSTRING".into(),
            args: vec![
                Expr::Column(ColumnRef::new("text")),
                Expr::Const(Const::Int(1)),
                Expr::Const(Const::Int(5)),
            ],
        };
        assert_eq!(render_scalar(&expr), "SUBSTRING(text, 1, 5)");
    }

    #[test]
    fn render_case_simple() {
        let expr = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::BinOp {
                    op: BinOp::Gt,
                    left: Box::new(Expr::Column(ColumnRef::new("age"))),
                    right: Box::new(Expr::Const(Const::Int(18))),
                },
                Expr::Const(Const::String("adult".into())),
            )],
            else_result: Some(Box::new(Expr::Const(Const::String("minor".into())))),
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("CASE"));
        assert!(sql.contains("WHEN"));
        assert!(sql.contains("ELSE"));
        assert!(sql.contains("END"));
    }

    #[test]
    fn render_case_no_else() {
        let expr = Expr::Case {
            operand: None,
            when_clauses: vec![(
                Expr::Const(Const::Bool(true)),
                Expr::Const(Const::Int(1)),
            )],
            else_result: None,
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("CASE"));
        assert!(!sql.contains("ELSE"));
    }

    #[test]
    fn render_case_with_operand() {
        let expr = Expr::Case {
            operand: Some(Box::new(Expr::Column(ColumnRef::new("status")))),
            when_clauses: vec![
                (Expr::Const(Const::String("active".into())), Expr::Const(Const::Int(1))),
                (Expr::Const(Const::String("inactive".into())), Expr::Const(Const::Int(0))),
            ],
            else_result: Some(Box::new(Expr::Const(Const::Int(-1)))),
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("CASE status"));
    }

    #[test]
    fn render_array() {
        let expr = Expr::Array(vec![
            Expr::Const(Const::Int(1)),
            Expr::Const(Const::Int(2)),
            Expr::Const(Const::Int(3)),
        ]);
        assert_eq!(render_scalar(&expr), "ARRAY[1, 2, 3]");
    }

    #[test]
    fn render_array_index() {
        let expr = Expr::ArrayIndex(
            Box::new(Expr::Column(ColumnRef::new("arr"))),
            Box::new(Expr::Const(Const::Int(0))),
        );
        assert_eq!(render_scalar(&expr), "arr[0]");
    }

    #[test]
    fn render_array_slice() {
        let expr = Expr::ArraySlice {
            array: Box::new(Expr::Column(ColumnRef::new("arr"))),
            start: Some(Box::new(Expr::Const(Const::Int(1)))),
            end: Some(Box::new(Expr::Const(Const::Int(5)))),
        };
        assert_eq!(render_scalar(&expr), "arr[1:5]");
    }

    #[test]
    fn render_array_slice_no_start() {
        let expr = Expr::ArraySlice {
            array: Box::new(Expr::Column(ColumnRef::new("arr"))),
            start: None,
            end: Some(Box::new(Expr::Const(Const::Int(5)))),
        };
        assert_eq!(render_scalar(&expr), "arr[:5]");
    }

    #[test]
    fn render_array_slice_no_end() {
        let expr = Expr::ArraySlice {
            array: Box::new(Expr::Column(ColumnRef::new("arr"))),
            start: Some(Box::new(Expr::Const(Const::Int(3)))),
            end: None,
        };
        assert_eq!(render_scalar(&expr), "arr[3:]");
    }

    #[test]
    fn render_field_access() {
        let expr = Expr::FieldAccess {
            expr: Box::new(Expr::Column(ColumnRef::new("user"))),
            field_name: "email".into(),
        };
        assert_eq!(render_scalar(&expr), "(user).email");
    }

    #[test]
    fn render_subquery_scalar() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::Scalar,
            query: Box::new(RelExpr::scan("users")),
            test_expr: None,
        };
        let sql = render_scalar(&expr);
        assert!(sql.starts_with('('));
        assert!(sql.ends_with(')'));
    }

    #[test]
    fn render_subquery_exists() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::Exists,
            query: Box::new(RelExpr::scan("orders")),
            test_expr: None,
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("EXISTS"));
    }

    #[test]
    fn render_subquery_in() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(RelExpr::scan("valid_ids")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("id")))),
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("IN"));
        assert!(sql.contains("id"));
    }

    #[test]
    fn render_subquery_in_no_test() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::In,
            query: Box::new(RelExpr::scan("valid_ids")),
            test_expr: None,
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("IN"));
    }

    #[test]
    fn render_subquery_any() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::Any,
            query: Box::new(RelExpr::scan("values")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("x")))),
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("ANY"));
    }

    #[test]
    fn render_subquery_all() {
        let expr = Expr::SubQuery {
            subquery_type: SubQueryType::All,
            query: Box::new(RelExpr::scan("values")),
            test_expr: Some(Box::new(Expr::Column(ColumnRef::new("x")))),
        };
        let sql = render_scalar(&expr);
        assert!(sql.contains("ALL"));
    }

    #[test]
    fn render_pattern_prev() {
        let expr = Expr::PatternPrev(
            Box::new(Expr::Column(ColumnRef::new("price"))),
            2,
        );
        assert_eq!(render_scalar(&expr), "PREV(price, 2)");
    }

    #[test]
    fn render_pattern_next() {
        let expr = Expr::PatternNext(
            Box::new(Expr::Column(ColumnRef::new("price"))),
            1,
        );
        assert_eq!(render_scalar(&expr), "NEXT(price, 1)");
    }

    #[test]
    fn render_pattern_first() {
        let expr = Expr::PatternFirst(
            Box::new(Expr::Column(ColumnRef::new("price"))),
            "A".into(),
        );
        assert_eq!(render_scalar(&expr), "FIRST(A.price)");
    }

    #[test]
    fn render_pattern_last() {
        let expr = Expr::PatternLast(
            Box::new(Expr::Column(ColumnRef::new("price"))),
            "B".into(),
        );
        assert_eq!(render_scalar(&expr), "LAST(B.price)");
    }

    #[test]
    fn render_pattern_classifier() {
        let expr = Expr::PatternClassifier;
        assert_eq!(render_scalar(&expr), "CLASSIFIER()");
    }

    #[test]
    fn render_pattern_match_number() {
        let expr = Expr::PatternMatchNumber;
        assert_eq!(render_scalar(&expr), "MATCH_NUMBER()");
    }

    #[test]
    fn render_const_float() {
        assert_eq!(render_const(&Const::Float(3.14)), "3.14");
    }

    #[test]
    fn render_const_string_with_quotes() {
        assert_eq!(render_const(&Const::String("it's".into())), "'it's'");
    }

    #[test]
    fn render_column_ref_qualified() {
        let col = ColumnRef::qualified("users", "id");
        assert_eq!(render_column_ref(&col), "users.id");
    }

    #[test]
    fn render_column_ref_simple() {
        let col = ColumnRef::new("name");
        assert_eq!(render_column_ref(&col), "name");
    }

    #[test]
    fn render_projection_with_aliases() {
        let cols = vec![
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("id")),
                alias: Some("user_id".into()),
            },
            ProjectionColumn {
                expr: Expr::Column(ColumnRef::new("name")),
                alias: None,
            },
        ];
        let sql = render_projection(&cols);
        assert!(sql.contains("id AS user_id"));
        assert!(sql.contains("name"));
    }

    #[test]
    fn render_aggregate_with_distinct() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(Expr::Column(ColumnRef::new("city"))),
            distinct: true,
            alias: Some("unique_cities".into()),
        }];
        let group_by = vec![];
        let sql = render_aggregate_select(&aggs, &group_by);
        assert!(sql.contains("COUNT(DISTINCT city)"));
        assert!(sql.contains("AS unique_cities"));
    }

    #[test]
    fn render_aggregate_with_group_by() {
        let aggs = vec![AggregateExpr {
            function: AggregateFunction::Sum,
            arg: Some(Expr::Column(ColumnRef::new("amount"))),
            distinct: false,
            alias: Some("total".into()),
        }];
        let group_by = vec![Expr::Column(ColumnRef::new("category"))];
        let sql = render_aggregate_select(&aggs, &group_by);
        assert!(sql.contains("SUM(amount)"));
        assert!(sql.contains("category"));
    }

    #[test]
    fn render_sort_key_nulls_first() {
        let key = SortKey {
            expr: Expr::Column(ColumnRef::new("score")),
            direction: SortDirection::Asc,
            nulls: NullOrdering::First,
        };
        assert_eq!(render_sort_key(&key), "score ASC NULLS FIRST");
    }

    #[test]
    fn render_scan_with_alias() {
        let expr = RelExpr::Scan {
            table: "users".into(),
            alias: Some("u".into()),
        };
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("users AS u"));
    }

    #[test]
    fn render_multiple_filters() {
        let expr = RelExpr::scan("users")
            .filter(Expr::BinOp {
                op: BinOp::Gt,
                left: Box::new(Expr::Column(ColumnRef::new("age"))),
                right: Box::new(Expr::Const(Const::Int(18))),
            })
            .filter(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(Expr::Column(ColumnRef::new("active"))),
                right: Box::new(Expr::Const(Const::Bool(true))),
            });
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("age > 18"));
        assert!(sql.contains("active = TRUE"));
        assert!(sql.contains("AND"));
    }

    #[test]
    fn render_zero_offset() {
        let expr = RelExpr::scan("users").limit(10, 0);
        let sql = SqlRenderer::render(&expr);
        assert!(sql.contains("LIMIT 10"));
        assert!(!sql.contains("OFFSET"));
    }

    #[test]
    fn render_empty_select() {
        let mut ctx = RenderContext::default();
        ctx.from = "users".into();
        let sql = ctx.finish();
        assert!(sql.contains("SELECT *"));
    }
}
