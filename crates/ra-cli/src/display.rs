//! Pretty-printing for relational expressions and query plans.

use ra_core::algebra::{AggregateFunction, RelExpr, SortDirection};
use ra_core::expr::{BinOp, Const, Expr, UnaryOp};

/// Format a relational expression as a tree structure.
///
/// # Example
///
/// ```text
/// Filter
///   predicate: age > 18
///   └─ Scan(users)
/// ```
pub fn format_plan_tree(expr: &RelExpr) -> String {
    let mut buf = String::new();
    format_plan_tree_impl(expr, &mut buf, "", true);
    buf
}

fn format_plan_tree_impl(expr: &RelExpr, buf: &mut String, prefix: &str, is_last: bool) {
    let connector = if is_last { "└─ " } else { "├─ " };
    let child_prefix_ext = if is_last { "   " } else { "│  " };

    match expr {
        RelExpr::Scan { table, alias } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Scan(");
            buf.push_str(table);
            if let Some(a) = alias {
                buf.push_str(" AS ");
                buf.push_str(a);
            }
            buf.push_str(")\n");
        }
        RelExpr::Filter { predicate, input } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Filter\n");
            buf.push_str(prefix);
            buf.push_str(&child_prefix_ext);
            buf.push_str("predicate: ");
            buf.push_str(&format_expr(predicate));
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Project { columns, input } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Project\n");
            buf.push_str(prefix);
            buf.push_str(&child_prefix_ext);
            buf.push_str("columns: ");
            for (i, col) in columns.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push_str(&format_expr(&col.expr));
                if let Some(a) = &col.alias {
                    buf.push_str(" AS ");
                    buf.push_str(a);
                }
            }
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Join {
            join_type,
            condition,
            left,
            right,
        } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str(&format!("{join_type} Join\n"));
            buf.push_str(prefix);
            buf.push_str(&child_prefix_ext);
            buf.push_str("condition: ");
            buf.push_str(&format_expr(condition));
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(left, buf, &child_prefix, false);
            format_plan_tree_impl(right, buf, &child_prefix, true);
        }
        RelExpr::Aggregate {
            group_by,
            aggregates,
            input,
        } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Aggregate\n");

            if !group_by.is_empty() {
                buf.push_str(prefix);
                buf.push_str(&child_prefix_ext);
                buf.push_str("group_by: ");
                for (i, expr) in group_by.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(&format_expr(expr));
                }
                buf.push('\n');
            }

            if !aggregates.is_empty() {
                buf.push_str(prefix);
                buf.push_str(&child_prefix_ext);
                buf.push_str("aggregates: ");
                for (i, agg) in aggregates.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(", ");
                    }
                    buf.push_str(&format_agg_function(agg.function));
                    buf.push('(');
                    if let Some(arg) = &agg.arg {
                        buf.push_str(&format_expr(arg));
                    }
                    buf.push(')');
                    if let Some(alias) = &agg.alias {
                        buf.push_str(" AS ");
                        buf.push_str(alias);
                    }
                }
                buf.push('\n');
            }

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Sort { keys, input } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Sort\n");
            buf.push_str(prefix);
            buf.push_str(&child_prefix_ext);
            buf.push_str("keys: ");
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                buf.push_str(&format_expr(&key.expr));
                buf.push(' ');
                buf.push_str(&format_sort_direction(key.direction));
            }
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Limit {
            count,
            offset,
            input,
        } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str(&format!("Limit(count={count}, offset={offset})\n"));

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Union { all, left, right } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Union");
            if *all {
                buf.push_str(" ALL");
            }
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(left, buf, &child_prefix, false);
            format_plan_tree_impl(right, buf, &child_prefix, true);
        }
        RelExpr::Intersect { all, left, right } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Intersect");
            if *all {
                buf.push_str(" ALL");
            }
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(left, buf, &child_prefix, false);
            format_plan_tree_impl(right, buf, &child_prefix, true);
        }
        RelExpr::Except { all, left, right } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Except");
            if *all {
                buf.push_str(" ALL");
            }
            buf.push('\n');

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(left, buf, &child_prefix, false);
            format_plan_tree_impl(right, buf, &child_prefix, true);
        }
        RelExpr::CTE {
            name,
            definition,
            body,
        } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str(&format!("CTE({name})\n"));

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(definition, buf, &child_prefix, false);
            format_plan_tree_impl(body, buf, &child_prefix, true);
        }
        RelExpr::Window { functions, input } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str(&format!(
                "Window({} function(s))\n",
                functions.len()
            ));

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Distinct { input } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str("Distinct\n");

            let child_prefix = format!("{prefix}{child_prefix_ext}");
            format_plan_tree_impl(input, buf, &child_prefix, true);
        }
        RelExpr::Values { rows } => {
            buf.push_str(prefix);
            buf.push_str(connector);
            buf.push_str(&format!("Values({} row(s))\n", rows.len()));
        }
    }
}

/// Format a scalar expression as a compact string.
fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Column(col) => {
            if let Some(table) = &col.table {
                format!("{table}.{}", col.column)
            } else {
                col.column.clone()
            }
        }
        Expr::Const(c) => format_const(c),
        Expr::BinOp { op, left, right } => {
            format!("({} {} {})", format_expr(left), format_binop(*op), format_expr(right))
        }
        Expr::UnaryOp { op, operand } => {
            format!("{}({})", format_unaryop(*op), format_expr(operand))
        }
        Expr::Function { name, args } => {
            let arg_strs: Vec<_> = args.iter().map(format_expr).collect();
            format!("{name}({})", arg_strs.join(", "))
        }
        Expr::Case {
            operand,
            when_clauses,
            else_result,
        } => {
            let mut s = "CASE".to_owned();
            if let Some(op) = operand {
                s.push(' ');
                s.push_str(&format_expr(op));
            }
            for (cond, result) in when_clauses {
                s.push_str(" WHEN ");
                s.push_str(&format_expr(cond));
                s.push_str(" THEN ");
                s.push_str(&format_expr(result));
            }
            if let Some(el) = else_result {
                s.push_str(" ELSE ");
                s.push_str(&format_expr(el));
            }
            s.push_str(" END");
            s
        }
        Expr::Cast { expr, target_type } => {
            format!("CAST({} AS {target_type})", format_expr(expr))
        }
    }
}

fn format_const(c: &Const) -> String {
    match c {
        Const::Null => "NULL".to_owned(),
        Const::Bool(b) => b.to_string().to_uppercase(),
        Const::Int(i) => i.to_string(),
        Const::Float(f) => f.to_string(),
        Const::String(s) => format!("'{s}'"),
    }
}

fn format_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Eq => "=",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::And => "AND",
        BinOp::Or => "OR",
    }
}

fn format_unaryop(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "NOT",
        UnaryOp::IsNull => "IS NULL",
        UnaryOp::IsNotNull => "IS NOT NULL",
        UnaryOp::Neg => "-",
    }
}

fn format_agg_function(func: AggregateFunction) -> &'static str {
    match func {
        AggregateFunction::Count => "COUNT",
        AggregateFunction::Sum => "SUM",
        AggregateFunction::Avg => "AVG",
        AggregateFunction::Min => "MIN",
        AggregateFunction::Max => "MAX",
        AggregateFunction::StdDev => "STDDEV",
        AggregateFunction::Variance => "VARIANCE",
        AggregateFunction::StringAgg => "STRING_AGG",
        AggregateFunction::ArrayAgg => "ARRAY_AGG",
    }
}

fn format_sort_direction(dir: SortDirection) -> &'static str {
    match dir {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_core::algebra::ProjectionColumn;
    use ra_core::expr::ColumnRef;

    #[test]
    fn format_simple_scan() {
        let plan = RelExpr::scan("users");
        let result = format_plan_tree(&plan);
        assert!(result.contains("Scan(users)"));
    }

    #[test]
    fn format_filter() {
        let plan = RelExpr::scan("users").filter(Expr::BinOp {
            op: BinOp::Gt,
            left: Box::new(Expr::Column(ColumnRef::new("age"))),
            right: Box::new(Expr::Const(Const::Int(18))),
        });
        let result = format_plan_tree(&plan);
        assert!(result.contains("Filter"));
        assert!(result.contains("age > 18"));
        assert!(result.contains("Scan(users)"));
    }

    #[test]
    fn format_project() {
        let plan = RelExpr::scan("users").project(vec![ProjectionColumn {
            expr: Expr::Column(ColumnRef::new("name")),
            alias: None,
        }]);
        let result = format_plan_tree(&plan);
        assert!(result.contains("Project"));
        assert!(result.contains("name"));
    }
}
