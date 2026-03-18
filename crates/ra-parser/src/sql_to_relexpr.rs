//! SQL to RelExpr converter using sqlparser.
//!
//! Supports SQL constructs:
//! - SELECT with projection list, DISTINCT
//! - FROM single table, INNER/LEFT/RIGHT/FULL/CROSS JOIN, subqueries
//! - WHERE with AND, OR, comparison operators
//! - GROUP BY with aggregates (COUNT, SUM, AVG, MIN, MAX, STDDEV, etc.)
//! - HAVING (post-aggregate filter)
//! - ORDER BY with ASC/DESC and NULLS FIRST/LAST
//! - LIMIT/OFFSET
//! - WITH/CTE (Common Table Expressions)
//! - Window functions (ROW_NUMBER, RANK, LAG, LEAD, etc.)
//! - UNION/INTERSECT/EXCEPT set operations

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey, WindowExpr,
    WindowFrame, WindowFrameBound, WindowFrameMode, WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};
use sqlparser::ast::{
    BinaryOperator, Distinct as SqlDistinct, Expr as SqlExpr,
    FunctionArg, FunctionArgExpr, FunctionArguments,
    GroupByExpr, Join as SqlJoin, JoinConstraint, JoinOperator,
    ObjectName, OrderByExpr as SqlOrderByExpr, Query, Select,
    SelectItem, SetExpr, SetOperator, SetQuantifier, Statement,
    TableFactor, TableWithJoins, UnaryOperator, Value,
    WindowFrameBound as SqlWindowFrameBound,
    WindowFrameUnits as SqlWindowFrameUnits,
    WindowSpec as SqlWindowSpec, WindowType as SqlWindowType,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use thiserror::Error;

/// Errors that can occur during SQL parsing and conversion.
#[derive(Debug, Error)]
pub enum SqlConversionError {
    /// SQL parsing failed.
    #[error("failed to parse SQL: {0}")]
    ParseError(String),

    /// Unsupported SQL construct.
    #[error("unsupported SQL feature: {0}")]
    UnsupportedFeature(String),

    /// Invalid SQL semantics.
    #[error("invalid SQL: {0}")]
    InvalidSql(String),
}

/// Parse SQL and convert to RelExpr.
///
/// # Errors
///
/// Returns error if SQL is invalid or contains unsupported features.
pub fn sql_to_relexpr(sql: &str) -> Result<RelExpr, SqlConversionError> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, sql)
        .map_err(|e| SqlConversionError::ParseError(e.to_string()))?;

    if statements.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "no SQL statement found".to_owned(),
        ));
    }

    if statements.len() > 1 {
        return Err(SqlConversionError::UnsupportedFeature(
            "multiple statements not supported".to_owned(),
        ));
    }

    match &statements[0] {
        Statement::Query(query) => convert_query(query),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only SELECT queries are supported".to_owned(),
        )),
    }
}

fn convert_query(query: &Query) -> Result<RelExpr, SqlConversionError> {
    // Handle WITH/CTE
    let mut plan = convert_query_body(query)?;

    if let Some(with) = &query.with {
        if with.recursive {
            return Err(SqlConversionError::UnsupportedFeature(
                "RECURSIVE CTE not yet supported".to_owned(),
            ));
        }
        // Wrap body in nested CTE nodes (innermost CTE first)
        for cte in with.cte_tables.iter().rev() {
            let cte_name = cte.alias.name.value.clone();
            let cte_def = convert_query(&cte.query)?;
            plan = RelExpr::CTE {
                name: cte_name,
                definition: Box::new(cte_def),
                body: Box::new(plan),
            };
        }
    }

    // Handle ORDER BY
    if let Some(order_by) = &query.order_by {
        if !order_by.exprs.is_empty() {
            let keys = convert_order_by_exprs(&order_by.exprs)?;
            plan = RelExpr::Sort {
                keys,
                input: Box::new(plan),
            };
        }
    }

    // Handle LIMIT/OFFSET
    if query.limit.is_some() || query.offset.is_some() {
        let count = match &query.limit {
            Some(expr) => extract_u64_from_expr(expr)?,
            None => u64::MAX,
        };
        let offset = match &query.offset {
            Some(off) => extract_u64_from_expr(&off.value)?,
            None => 0,
        };
        plan = RelExpr::Limit {
            count,
            offset,
            input: Box::new(plan),
        };
    }

    Ok(plan)
}

fn convert_query_body(
    query: &Query,
) -> Result<RelExpr, SqlConversionError> {
    match &*query.body {
        SetExpr::Select(select) => convert_select(select),
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => convert_set_operation(op, set_quantifier, left, right),
        SetExpr::Query(inner) => convert_query(inner),
        SetExpr::Values(values) => {
            let mut rows = Vec::new();
            for row in &values.rows {
                let mut exprs = Vec::new();
                for val in row {
                    exprs.push(convert_expr(val)?);
                }
                rows.push(exprs);
            }
            Ok(RelExpr::Values { rows })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported query body type".to_owned(),
        )),
    }
}

fn convert_set_operation(
    op: &SetOperator,
    quantifier: &SetQuantifier,
    left: &SetExpr,
    right: &SetExpr,
) -> Result<RelExpr, SqlConversionError> {
    let left_plan = convert_set_expr(left)?;
    let right_plan = convert_set_expr(right)?;
    let all = matches!(
        quantifier,
        SetQuantifier::All | SetQuantifier::AllByName
    );

    match op {
        SetOperator::Union => Ok(RelExpr::Union {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
        SetOperator::Intersect => Ok(RelExpr::Intersect {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
        SetOperator::Except => Ok(RelExpr::Except {
            all,
            left: Box::new(left_plan),
            right: Box::new(right_plan),
        }),
    }
}

fn convert_set_expr(
    set_expr: &SetExpr,
) -> Result<RelExpr, SqlConversionError> {
    match set_expr {
        SetExpr::Select(select) => convert_select(select),
        SetExpr::SetOperation {
            op,
            set_quantifier,
            left,
            right,
        } => convert_set_operation(op, set_quantifier, left, right),
        SetExpr::Query(inner) => convert_query(inner),
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported set expression type".to_owned(),
        )),
    }
}

fn convert_select(
    select: &Select,
) -> Result<RelExpr, SqlConversionError> {
    if !select.cluster_by.is_empty()
        || !select.distribute_by.is_empty()
        || !select.sort_by.is_empty()
    {
        return Err(SqlConversionError::UnsupportedFeature(
            "CLUSTER BY/DISTRIBUTE BY/SORT BY not supported".to_owned(),
        ));
    }

    // Start with FROM clause
    let mut plan = convert_from(&select.from)?;

    // Apply WHERE clause
    if let Some(ref where_expr) = select.selection {
        let predicate = convert_expr(where_expr)?;
        plan = RelExpr::Filter {
            predicate,
            input: Box::new(plan),
        };
    }

    // Check for aggregates and GROUP BY
    let has_group_by = !matches!(
        select.group_by,
        GroupByExpr::All(ref exprs) if exprs.is_empty()
    );
    let has_aggregates = select.projection.iter().any(|item| match item {
        SelectItem::UnnamedExpr(expr)
        | SelectItem::ExprWithAlias { expr, .. } => {
            is_aggregate(expr)
        }
        _ => false,
    });
    let has_having = select.having.is_some();

    if has_group_by || has_aggregates || has_having {
        plan = convert_group_by(plan, select)?;
    }

    // Apply HAVING as post-aggregate filter
    if let Some(ref having_expr) = select.having {
        let predicate = convert_having_expr(having_expr)?;
        plan = RelExpr::Filter {
            predicate,
            input: Box::new(plan),
        };
    }

    // Extract window functions from projection
    let window_fns = extract_window_functions(&select.projection)?;
    if !window_fns.is_empty() {
        plan = RelExpr::Window {
            functions: window_fns,
            input: Box::new(plan),
        };
    }

    // Apply projection (SELECT list)
    plan = convert_projection(plan, &select.projection)?;

    // Apply DISTINCT
    if let Some(ref distinct) = select.distinct {
        match distinct {
            SqlDistinct::Distinct | SqlDistinct::On(_) => {
                plan = RelExpr::Distinct {
                    input: Box::new(plan),
                };
            }
        }
    }

    Ok(plan)
}

fn convert_from(
    from: &[TableWithJoins],
) -> Result<RelExpr, SqlConversionError> {
    if from.is_empty() {
        return Err(SqlConversionError::InvalidSql(
            "SELECT without FROM not supported".to_owned(),
        ));
    }

    // Handle multiple FROM items as implicit cross joins
    let mut plan = convert_table_with_joins(&from[0])?;
    for twj in &from[1..] {
        let right = convert_table_with_joins(twj)?;
        plan = RelExpr::Join {
            join_type: JoinType::Cross,
            condition: Expr::Const(Const::Bool(true)),
            left: Box::new(plan),
            right: Box::new(right),
        };
    }

    Ok(plan)
}

fn convert_table_with_joins(
    twj: &TableWithJoins,
) -> Result<RelExpr, SqlConversionError> {
    let mut plan = convert_table_factor(&twj.relation)?;

    for join in &twj.joins {
        plan = convert_join(plan, join)?;
    }

    Ok(plan)
}

fn convert_table_factor(
    table: &TableFactor,
) -> Result<RelExpr, SqlConversionError> {
    match table {
        TableFactor::Table { name, alias, .. } => {
            let table_name = object_name_to_string(name);
            let alias_name =
                alias.as_ref().map(|a| a.name.value.clone());

            Ok(RelExpr::Scan {
                table: table_name,
                alias: alias_name,
            })
        }
        TableFactor::Derived {
            subquery, alias, ..
        } => {
            let mut sub_plan = convert_query(subquery)?;
            // If there's an alias, wrap in a scan-like reference
            if let Some(alias) = alias {
                sub_plan = RelExpr::Project {
                    columns: vec![ProjectionColumn {
                        expr: Expr::Column(ColumnRef::new("*")),
                        alias: Some(alias.name.value.clone()),
                    }],
                    input: Box::new(sub_plan),
                };
            }
            Ok(sub_plan)
        }
        TableFactor::TableFunction { .. } => {
            Err(SqlConversionError::UnsupportedFeature(
                "table functions not supported".to_owned(),
            ))
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "unsupported table factor".to_owned(),
        )),
    }
}

fn convert_join(
    left: RelExpr,
    join: &SqlJoin,
) -> Result<RelExpr, SqlConversionError> {
    let right = convert_table_factor(&join.relation)?;

    let (join_type, constraint) = match &join.join_operator {
        JoinOperator::Inner(c) => (JoinType::Inner, c),
        JoinOperator::LeftOuter(c) => (JoinType::LeftOuter, c),
        JoinOperator::RightOuter(c) => (JoinType::RightOuter, c),
        JoinOperator::FullOuter(c) => (JoinType::FullOuter, c),
        JoinOperator::CrossJoin => {
            return Ok(RelExpr::Join {
                join_type: JoinType::Cross,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(left),
                right: Box::new(right),
            })
        }
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                "unsupported join type".to_owned(),
            ))
        }
    };

    let condition = match constraint {
        JoinConstraint::On(expr) => convert_expr(expr)?,
        JoinConstraint::Using(cols) => {
            // Convert USING(a, b) to a.col = b.col AND ...
            build_using_condition(cols)?
        }
        JoinConstraint::Natural => {
            return Err(SqlConversionError::UnsupportedFeature(
                "NATURAL JOIN not supported".to_owned(),
            ))
        }
        JoinConstraint::None => Expr::Const(Const::Bool(true)),
    };

    Ok(RelExpr::Join {
        join_type,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    })
}

fn build_using_condition(
    cols: &[sqlparser::ast::Ident],
) -> Result<Expr, SqlConversionError> {
    if cols.is_empty() {
        return Ok(Expr::Const(Const::Bool(true)));
    }

    let mut conditions: Vec<Expr> = Vec::new();
    for col in cols {
        let col_name = &col.value;
        conditions.push(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(Expr::Column(ColumnRef::new(col_name))),
            right: Box::new(Expr::Column(ColumnRef::new(col_name))),
        });
    }

    let mut result = conditions.remove(0);
    for cond in conditions {
        result = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(result),
            right: Box::new(cond),
        };
    }
    Ok(result)
}

fn convert_group_by(
    input: RelExpr,
    select: &Select,
) -> Result<RelExpr, SqlConversionError> {
    let group_by_exprs = match &select.group_by {
        GroupByExpr::All(_) => vec![],
        GroupByExpr::Expressions(exprs, _) => exprs.clone(),
    };

    let group_by: Result<Vec<_>, _> =
        group_by_exprs.iter().map(convert_expr).collect();
    let group_by = group_by?;

    // Extract aggregates from projection
    let mut aggregates = Vec::new();
    for item in &select.projection {
        if let SelectItem::UnnamedExpr(expr)
        | SelectItem::ExprWithAlias { expr, .. } = item
        {
            if let Some(agg) = try_extract_aggregate(expr, item)? {
                aggregates.push(agg);
            }
        }
    }

    // Also extract aggregates from HAVING clause
    if let Some(ref having) = select.having {
        extract_having_aggregates(having, &mut aggregates)?;
    }

    if group_by.is_empty() && aggregates.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Aggregate {
        group_by,
        aggregates,
        input: Box::new(input),
    })
}

fn extract_having_aggregates(
    expr: &SqlExpr,
    aggregates: &mut Vec<AggregateExpr>,
) -> Result<(), SqlConversionError> {
    match expr {
        SqlExpr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            if let Some(agg_func) = parse_agg_function(&func_name) {
                let arg = extract_single_func_arg(&func.args)?;
                let distinct = match &func.args {
                    FunctionArguments::List(list) => {
                        list.duplicate_treatment.is_some()
                    }
                    _ => false,
                };
                // Avoid duplicate aggregates
                let already_exists = aggregates.iter().any(|a| {
                    a.function == agg_func && a.arg == arg
                });
                if !already_exists {
                    aggregates.push(AggregateExpr {
                        function: agg_func,
                        arg,
                        distinct,
                        alias: None,
                    });
                }
            }
        }
        SqlExpr::BinaryOp { left, right, .. } => {
            extract_having_aggregates(left, aggregates)?;
            extract_having_aggregates(right, aggregates)?;
        }
        SqlExpr::UnaryOp { expr, .. } => {
            extract_having_aggregates(expr, aggregates)?;
        }
        SqlExpr::Nested(inner) => {
            extract_having_aggregates(inner, aggregates)?;
        }
        _ => {}
    }
    Ok(())
}

fn try_extract_aggregate(
    expr: &SqlExpr,
    item: &SelectItem,
) -> Result<Option<AggregateExpr>, SqlConversionError> {
    match expr {
        SqlExpr::Function(func) if func.over.is_none() => {
            let func_name = func.name.to_string().to_uppercase();
            let agg_func = match parse_agg_function(&func_name) {
                Some(f) => f,
                None => return Ok(None),
            };

            let arg = extract_single_func_arg(&func.args)?;

            let alias = match item {
                SelectItem::ExprWithAlias { alias, .. } => {
                    Some(alias.value.clone())
                }
                _ => None,
            };

            let distinct = match &func.args {
                FunctionArguments::List(list) => {
                    list.duplicate_treatment.is_some()
                }
                _ => false,
            };

            Ok(Some(AggregateExpr {
                function: agg_func,
                arg,
                distinct,
                alias,
            }))
        }
        _ => Ok(None),
    }
}

fn parse_agg_function(name: &str) -> Option<AggregateFunction> {
    match name {
        "COUNT" => Some(AggregateFunction::Count),
        "SUM" => Some(AggregateFunction::Sum),
        "AVG" => Some(AggregateFunction::Avg),
        "MIN" => Some(AggregateFunction::Min),
        "MAX" => Some(AggregateFunction::Max),
        "STDDEV" | "STDDEV_POP" | "STDDEV_SAMP" => {
            Some(AggregateFunction::StdDev)
        }
        "VARIANCE" | "VAR_POP" | "VAR_SAMP" => {
            Some(AggregateFunction::Variance)
        }
        "STRING_AGG" | "GROUP_CONCAT" => {
            Some(AggregateFunction::StringAgg)
        }
        "ARRAY_AGG" => Some(AggregateFunction::ArrayAgg),
        _ => None,
    }
}

fn extract_single_func_arg(
    args: &FunctionArguments,
) -> Result<Option<Expr>, SqlConversionError> {
    let func_args = match args {
        FunctionArguments::None => return Ok(None),
        FunctionArguments::Subquery(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "subquery in function not supported".to_owned(),
            ))
        }
        FunctionArguments::List(list) => &list.args,
    };

    if func_args.is_empty() {
        return Ok(None);
    }

    if func_args.len() == 1 {
        match &func_args[0] {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                Ok(Some(convert_expr(e)?))
            }
            FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => Ok(None),
            _ => Err(SqlConversionError::UnsupportedFeature(
                "complex function arguments not supported".to_owned(),
            )),
        }
    } else {
        // For multi-arg functions, take just the first arg
        match &func_args[0] {
            FunctionArg::Unnamed(FunctionArgExpr::Expr(e)) => {
                Ok(Some(convert_expr(e)?))
            }
            _ => Err(SqlConversionError::UnsupportedFeature(
                "complex multi-argument functions not supported"
                    .to_owned(),
            )),
        }
    }
}

fn extract_window_functions(
    projection: &[SelectItem],
) -> Result<Vec<WindowExpr>, SqlConversionError> {
    let mut window_fns = Vec::new();

    for item in projection {
        let (expr, alias) = match item {
            SelectItem::UnnamedExpr(e) => (e, None),
            SelectItem::ExprWithAlias { expr, alias } => {
                (expr, Some(alias.value.clone()))
            }
            _ => continue,
        };

        if let SqlExpr::Function(func) = expr {
            if let Some(over) = &func.over {
                let wf = convert_window_function(func, over, alias)?;
                window_fns.push(wf);
            }
        }
    }

    Ok(window_fns)
}

fn convert_window_function(
    func: &sqlparser::ast::Function,
    over: &SqlWindowType,
    alias: Option<String>,
) -> Result<WindowExpr, SqlConversionError> {
    let func_name = func.name.to_string().to_uppercase();
    let window_fn = match func_name.as_str() {
        "ROW_NUMBER" => WindowFunction::RowNumber,
        "RANK" => WindowFunction::Rank,
        "DENSE_RANK" => WindowFunction::DenseRank,
        "PERCENT_RANK" => WindowFunction::PercentRank,
        "NTILE" => WindowFunction::Ntile,
        "LAG" => WindowFunction::Lag,
        "LEAD" => WindowFunction::Lead,
        "FIRST_VALUE" => WindowFunction::FirstValue,
        "LAST_VALUE" => WindowFunction::LastValue,
        "NTH_VALUE" => WindowFunction::NthValue,
        "AVG" => WindowFunction::Avg,
        "SUM" => WindowFunction::Sum,
        "COUNT" => WindowFunction::Count,
        "MIN" => WindowFunction::Min,
        "MAX" => WindowFunction::Max,
        _ => {
            return Err(SqlConversionError::UnsupportedFeature(
                format!("window function {func_name} not supported"),
            ))
        }
    };

    let arg = extract_single_func_arg(&func.args)?;

    let spec = match over {
        SqlWindowType::WindowSpec(spec) => spec,
        SqlWindowType::NamedWindow(_) => {
            return Err(SqlConversionError::UnsupportedFeature(
                "named windows not yet supported".to_owned(),
            ))
        }
    };

    let (partition_by, order_by, frame) =
        convert_window_spec(spec)?;

    Ok(WindowExpr {
        function: window_fn,
        arg,
        partition_by,
        order_by,
        frame,
        alias,
    })
}

fn convert_window_spec(
    spec: &SqlWindowSpec,
) -> Result<(Vec<Expr>, Vec<SortKey>, Option<WindowFrame>), SqlConversionError>
{
    let partition_by: Result<Vec<_>, _> =
        spec.partition_by.iter().map(convert_expr).collect();
    let partition_by = partition_by?;

    let order_by = convert_order_by_exprs(&spec.order_by)?;

    let frame = match &spec.window_frame {
        Some(wf) => Some(convert_window_frame(wf)?),
        None => None,
    };

    Ok((partition_by, order_by, frame))
}

fn convert_window_frame(
    wf: &sqlparser::ast::WindowFrame,
) -> Result<WindowFrame, SqlConversionError> {
    let mode = match wf.units {
        SqlWindowFrameUnits::Rows => WindowFrameMode::Rows,
        SqlWindowFrameUnits::Range => WindowFrameMode::Range,
        SqlWindowFrameUnits::Groups => WindowFrameMode::Groups,
    };

    let start = convert_frame_bound(&wf.start_bound)?;
    let end = match &wf.end_bound {
        Some(bound) => convert_frame_bound(bound)?,
        None => WindowFrameBound::CurrentRow,
    };

    Ok(WindowFrame { mode, start, end })
}

fn convert_frame_bound(
    bound: &SqlWindowFrameBound,
) -> Result<WindowFrameBound, SqlConversionError> {
    match bound {
        SqlWindowFrameBound::CurrentRow => {
            Ok(WindowFrameBound::CurrentRow)
        }
        SqlWindowFrameBound::Preceding(None) => {
            Ok(WindowFrameBound::UnboundedPreceding)
        }
        SqlWindowFrameBound::Preceding(Some(expr)) => {
            let n = extract_u64_from_expr(expr)?;
            Ok(WindowFrameBound::Preceding(n))
        }
        SqlWindowFrameBound::Following(None) => {
            Ok(WindowFrameBound::UnboundedFollowing)
        }
        SqlWindowFrameBound::Following(Some(expr)) => {
            let n = extract_u64_from_expr(expr)?;
            Ok(WindowFrameBound::Following(n))
        }
    }
}

fn convert_order_by_exprs(
    exprs: &[SqlOrderByExpr],
) -> Result<Vec<SortKey>, SqlConversionError> {
    let mut keys = Vec::new();
    for ob in exprs {
        let expr = convert_expr(&ob.expr)?;
        let direction = match ob.asc {
            Some(true) | None => SortDirection::Asc,
            Some(false) => SortDirection::Desc,
        };
        let nulls = match ob.nulls_first {
            Some(true) => NullOrdering::First,
            Some(false) => NullOrdering::Last,
            None => match direction {
                SortDirection::Asc => NullOrdering::Last,
                SortDirection::Desc => NullOrdering::First,
            },
        };
        keys.push(SortKey {
            expr,
            direction,
            nulls,
        });
    }
    Ok(keys)
}

fn convert_having_expr(
    expr: &SqlExpr,
) -> Result<Expr, SqlConversionError> {
    match expr {
        SqlExpr::BinaryOp { left, op, right } => {
            let bin_op = convert_binary_op(op)?;
            Ok(Expr::BinOp {
                op: bin_op,
                left: Box::new(convert_having_expr(left)?),
                right: Box::new(convert_having_expr(right)?),
            })
        }
        SqlExpr::Function(func) => {
            let func_name = func.name.to_string().to_uppercase();
            // Represent aggregate references in HAVING as function
            // calls in the expression tree
            let arg = extract_single_func_arg(&func.args)?;
            let args = match arg {
                Some(a) => vec![a],
                None => vec![],
            };
            Ok(Expr::Function {
                name: func_name,
                args,
            })
        }
        SqlExpr::Nested(inner) => convert_having_expr(inner),
        _ => convert_expr(expr),
    }
}

fn convert_projection(
    input: RelExpr,
    projection: &[SelectItem],
) -> Result<RelExpr, SqlConversionError> {
    if projection.len() == 1 {
        if let SelectItem::Wildcard(_) = projection[0] {
            return Ok(input);
        }
    }

    let mut columns = Vec::new();

    for item in projection {
        match item {
            SelectItem::UnnamedExpr(expr) => {
                if !is_aggregate(expr) && !is_window_function(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: None,
                    });
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                if !is_aggregate(expr) && !is_window_function(expr) {
                    columns.push(ProjectionColumn {
                        expr: convert_expr(expr)?,
                        alias: Some(alias.value.clone()),
                    });
                }
            }
            SelectItem::QualifiedWildcard(obj_name, _) => {
                return Err(SqlConversionError::UnsupportedFeature(
                    format!(
                        "qualified wildcard {obj_name}.* \
                         not yet supported"
                    ),
                ))
            }
            SelectItem::Wildcard(_) => {
                return Err(SqlConversionError::UnsupportedFeature(
                    "wildcard in multi-column projection \
                     not yet supported"
                        .to_owned(),
                ))
            }
        }
    }

    if columns.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Project {
        columns,
        input: Box::new(input),
    })
}

fn is_aggregate(expr: &SqlExpr) -> bool {
    match expr {
        SqlExpr::Function(func) if func.over.is_none() => {
            let name = func.name.to_string().to_uppercase();
            parse_agg_function(&name).is_some()
        }
        _ => false,
    }
}

fn is_window_function(expr: &SqlExpr) -> bool {
    matches!(expr, SqlExpr::Function(func) if func.over.is_some())
}

fn convert_expr(expr: &SqlExpr) -> Result<Expr, SqlConversionError> {
    match expr {
        SqlExpr::Identifier(ident) => {
            Ok(Expr::Column(ColumnRef::new(&ident.value)))
        }
        SqlExpr::CompoundIdentifier(idents) => {
            if idents.len() == 2 {
                Ok(Expr::Column(ColumnRef::qualified(
                    &idents[0].value,
                    &idents[1].value,
                )))
            } else {
                Err(SqlConversionError::UnsupportedFeature(format!(
                    "complex identifier with {} parts not supported",
                    idents.len()
                )))
            }
        }
        SqlExpr::Value(val) => convert_value(val),
        SqlExpr::BinaryOp { left, op, right } => {
            let bin_op = convert_binary_op(op)?;
            Ok(Expr::BinOp {
                op: bin_op,
                left: Box::new(convert_expr(left)?),
                right: Box::new(convert_expr(right)?),
            })
        }
        SqlExpr::UnaryOp { op, expr } => {
            let unary_op = convert_unary_op(op)?;
            Ok(Expr::UnaryOp {
                op: unary_op,
                operand: Box::new(convert_expr(expr)?),
            })
        }
        SqlExpr::Nested(inner) => convert_expr(inner),
        SqlExpr::IsNull(expr) => Ok(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNull,
            operand: Box::new(convert_expr(expr)?),
        }),
        SqlExpr::IsNotNull(expr) => Ok(Expr::UnaryOp {
            op: ra_core::expr::UnaryOp::IsNotNull,
            operand: Box::new(convert_expr(expr)?),
        }),
        SqlExpr::Function(func) => {
            let name = func.name.to_string().to_uppercase();
            let arg = extract_single_func_arg(&func.args)?;
            let args = match arg {
                Some(a) => vec![a],
                None => vec![],
            };
            Ok(Expr::Function { name, args })
        }
        SqlExpr::Subquery(query) => {
            // Represent subquery as a function node for now
            let _plan = convert_query(query)?;
            Ok(Expr::Function {
                name: "SUBQUERY".to_owned(),
                args: vec![],
            })
        }
        SqlExpr::InSubquery {
            expr, subquery, ..
        } => {
            let left = convert_expr(expr)?;
            let _plan = convert_query(subquery)?;
            Ok(Expr::Function {
                name: "IN_SUBQUERY".to_owned(),
                args: vec![left],
            })
        }
        SqlExpr::Exists { subquery, .. } => {
            let _plan = convert_query(subquery)?;
            Ok(Expr::Function {
                name: "EXISTS".to_owned(),
                args: vec![],
            })
        }
        SqlExpr::Between {
            expr, low, high, ..
        } => {
            let col = convert_expr(expr)?;
            let low_expr = convert_expr(low)?;
            let high_expr = convert_expr(high)?;
            Ok(Expr::BinOp {
                op: BinOp::And,
                left: Box::new(Expr::BinOp {
                    op: BinOp::Ge,
                    left: Box::new(col.clone()),
                    right: Box::new(low_expr),
                }),
                right: Box::new(Expr::BinOp {
                    op: BinOp::Le,
                    left: Box::new(col),
                    right: Box::new(high_expr),
                }),
            })
        }
        SqlExpr::Cast {
            expr, data_type, ..
        } => Ok(Expr::Cast {
            expr: Box::new(convert_expr(expr)?),
            target_type: data_type.to_string(),
        }),
        SqlExpr::Case {
            operand,
            conditions,
            results,
            else_result,
        } => {
            let op = match operand {
                Some(e) => Some(Box::new(convert_expr(e)?)),
                None => None,
            };
            let mut when_clauses = Vec::new();
            for (cond, result) in
                conditions.iter().zip(results.iter())
            {
                when_clauses.push((
                    convert_expr(cond)?,
                    convert_expr(result)?,
                ));
            }
            let else_r = match else_result {
                Some(e) => Some(Box::new(convert_expr(e)?)),
                None => None,
            };
            Ok(Expr::Case {
                operand: op,
                when_clauses,
                else_result: else_r,
            })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "expression type not yet supported: {expr}"
        ))),
    }
}

fn convert_value(val: &Value) -> Result<Expr, SqlConversionError> {
    match val {
        Value::Number(n, _) => {
            if let Ok(i) = n.parse::<i64>() {
                Ok(Expr::Const(Const::Int(i)))
            } else if let Ok(f) = n.parse::<f64>() {
                Ok(Expr::Const(Const::Float(f)))
            } else {
                Err(SqlConversionError::InvalidSql(format!(
                    "invalid number: {n}"
                )))
            }
        }
        Value::SingleQuotedString(s)
        | Value::DoubleQuotedString(s) => {
            Ok(Expr::Const(Const::String(s.clone())))
        }
        Value::Boolean(b) => Ok(Expr::Const(Const::Bool(*b))),
        Value::Null => Ok(Expr::Const(Const::Null)),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "value type not supported: {val:?}"
        ))),
    }
}

fn convert_binary_op(
    op: &BinaryOperator,
) -> Result<BinOp, SqlConversionError> {
    match op {
        BinaryOperator::Plus => Ok(BinOp::Add),
        BinaryOperator::Minus => Ok(BinOp::Sub),
        BinaryOperator::Multiply => Ok(BinOp::Mul),
        BinaryOperator::Divide => Ok(BinOp::Div),
        BinaryOperator::Eq => Ok(BinOp::Eq),
        BinaryOperator::NotEq => Ok(BinOp::Ne),
        BinaryOperator::Lt => Ok(BinOp::Lt),
        BinaryOperator::LtEq => Ok(BinOp::Le),
        BinaryOperator::Gt => Ok(BinOp::Gt),
        BinaryOperator::GtEq => Ok(BinOp::Ge),
        BinaryOperator::And => Ok(BinOp::And),
        BinaryOperator::Or => Ok(BinOp::Or),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "binary operator not supported: {op:?}"
        ))),
    }
}

fn convert_unary_op(
    op: &UnaryOperator,
) -> Result<ra_core::expr::UnaryOp, SqlConversionError> {
    match op {
        UnaryOperator::Not => Ok(ra_core::expr::UnaryOp::Not),
        UnaryOperator::Minus => Ok(ra_core::expr::UnaryOp::Neg),
        _ => Err(SqlConversionError::UnsupportedFeature(format!(
            "unary operator not supported: {op:?}"
        ))),
    }
}

fn extract_u64_from_expr(
    expr: &SqlExpr,
) -> Result<u64, SqlConversionError> {
    match expr {
        SqlExpr::Value(Value::Number(n, _)) => {
            n.parse::<u64>().map_err(|_| {
                SqlConversionError::InvalidSql(format!(
                    "expected unsigned integer, got: {n}"
                ))
            })
        }
        _ => Err(SqlConversionError::UnsupportedFeature(
            "only literal integers supported for LIMIT/OFFSET"
                .to_owned(),
        )),
    }
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .iter()
        .map(|i| i.value.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Existing tests (preserved) ----

    #[test]
    fn test_simple_select() {
        let sql = "SELECT * FROM users";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_where() {
        let sql = "SELECT * FROM users WHERE age > 18";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_join() {
        let sql = "SELECT * FROM orders o \
                   JOIN customers c ON o.customer_id = c.id";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    #[test]
    fn test_select_with_aggregate() {
        let sql = "SELECT region, COUNT(*), SUM(amount) \
                   FROM orders GROUP BY region";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok());
    }

    // ---- DISTINCT tests ----

    #[test]
    fn test_select_distinct() {
        let sql = "SELECT DISTINCT name FROM users";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(
            matches!(result, RelExpr::Distinct { .. }),
            "expected Distinct at top level"
        );
    }

    #[test]
    fn test_select_distinct_multiple_cols() {
        let sql =
            "SELECT DISTINCT dept_id, job_title FROM employees";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(matches!(result, RelExpr::Distinct { .. }));
    }

    // ---- ORDER BY tests ----

    #[test]
    fn test_order_by_asc() {
        let sql = "SELECT * FROM users ORDER BY name ASC";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Sort { keys, .. } = &result {
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].direction, SortDirection::Asc);
        } else {
            panic!("expected Sort at top level");
        }
    }

    #[test]
    fn test_order_by_desc() {
        let sql = "SELECT * FROM users ORDER BY age DESC";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Sort { keys, .. } = &result {
            assert_eq!(keys[0].direction, SortDirection::Desc);
        } else {
            panic!("expected Sort at top level");
        }
    }

    #[test]
    fn test_order_by_multiple() {
        let sql =
            "SELECT * FROM users ORDER BY dept ASC, name DESC";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Sort { keys, .. } = &result {
            assert_eq!(keys.len(), 2);
            assert_eq!(keys[0].direction, SortDirection::Asc);
            assert_eq!(keys[1].direction, SortDirection::Desc);
        } else {
            panic!("expected Sort at top level");
        }
    }

    #[test]
    fn test_order_by_nulls() {
        let sql =
            "SELECT * FROM users ORDER BY name ASC NULLS FIRST";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Sort { keys, .. } = &result {
            assert_eq!(keys[0].nulls, NullOrdering::First);
        } else {
            panic!("expected Sort at top level");
        }
    }

    // ---- LIMIT/OFFSET tests ----

    #[test]
    fn test_limit() {
        let sql = "SELECT * FROM users LIMIT 10";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Limit {
            count, offset, ..
        } = &result
        {
            assert_eq!(*count, 10);
            assert_eq!(*offset, 0);
        } else {
            panic!("expected Limit at top level");
        }
    }

    #[test]
    fn test_limit_offset() {
        let sql = "SELECT * FROM users LIMIT 10 OFFSET 20";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Limit {
            count, offset, ..
        } = &result
        {
            assert_eq!(*count, 10);
            assert_eq!(*offset, 20);
        } else {
            panic!("expected Limit at top level");
        }
    }

    #[test]
    fn test_order_by_with_limit() {
        let sql =
            "SELECT * FROM users ORDER BY name LIMIT 5";
        let result = sql_to_relexpr(sql).expect("should parse");
        // Should be Limit(Sort(...))
        if let RelExpr::Limit { input, count, .. } = &result {
            assert_eq!(*count, 5);
            assert!(matches!(input.as_ref(), RelExpr::Sort { .. }));
        } else {
            panic!("expected Limit(Sort(...))");
        }
    }

    // ---- HAVING tests ----

    #[test]
    fn test_having() {
        let sql = "SELECT dept_id, COUNT(*) \
                   FROM employees \
                   GROUP BY dept_id \
                   HAVING COUNT(*) > 5";
        let result = sql_to_relexpr(sql).expect("should parse");
        // Should contain a Filter (HAVING) over Aggregate
        fn has_filter(r: &RelExpr) -> bool {
            match r {
                RelExpr::Filter { .. } => true,
                _ => r.children().iter().any(|c| has_filter(c)),
            }
        }
        assert!(has_filter(&result), "expected Filter for HAVING");
    }

    #[test]
    fn test_having_with_group_by() {
        let sql = "SELECT region, SUM(amount) as total \
                   FROM orders \
                   GROUP BY region \
                   HAVING SUM(amount) > 1000";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn has_aggregate(r: &RelExpr) -> bool {
            match r {
                RelExpr::Aggregate { .. } => true,
                _ => r.children().iter().any(|c| has_aggregate(c)),
            }
        }
        assert!(
            has_aggregate(&result),
            "expected Aggregate node"
        );
    }

    // ---- CTE tests ----

    #[test]
    fn test_simple_cte() {
        let sql = "WITH active AS (SELECT * FROM users WHERE active = true) \
                   SELECT * FROM active";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::CTE { name, .. } = &result {
            assert_eq!(name, "active");
        } else {
            panic!("expected CTE at top level");
        }
    }

    #[test]
    fn test_multiple_ctes() {
        let sql = "WITH \
                     a AS (SELECT * FROM t1), \
                     b AS (SELECT * FROM t2) \
                   SELECT * FROM a";
        let result = sql_to_relexpr(sql).expect("should parse");
        // Outermost should be CTE 'a' wrapping CTE 'b'
        if let RelExpr::CTE {
            name, body, ..
        } = &result
        {
            assert_eq!(name, "a");
            assert!(matches!(body.as_ref(), RelExpr::CTE { .. }));
        } else {
            panic!("expected nested CTEs");
        }
    }

    // ---- Subquery tests ----

    #[test]
    fn test_subquery_in_from() {
        let sql =
            "SELECT * FROM (SELECT id, name FROM users) t";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok(), "subquery in FROM should parse");
    }

    #[test]
    fn test_subquery_in_where() {
        let sql = "SELECT * FROM orders \
                   WHERE customer_id IN \
                   (SELECT id FROM customers WHERE active = true)";
        let result = sql_to_relexpr(sql);
        assert!(
            result.is_ok(),
            "subquery in WHERE should parse"
        );
    }

    #[test]
    fn test_exists_subquery() {
        let sql = "SELECT * FROM customers c \
                   WHERE EXISTS \
                   (SELECT 1 FROM orders o WHERE o.cust_id = c.id)";
        let result = sql_to_relexpr(sql);
        assert!(
            result.is_ok(),
            "EXISTS subquery should parse"
        );
    }

    // ---- JOIN type tests ----

    #[test]
    fn test_left_join() {
        let sql = "SELECT * FROM orders o \
                   LEFT JOIN customers c ON o.cust_id = c.id";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Join { join_type, .. } = &result {
            assert_eq!(*join_type, JoinType::LeftOuter);
        } else {
            panic!("expected Join at top level");
        }
    }

    #[test]
    fn test_right_join() {
        let sql = "SELECT * FROM orders o \
                   RIGHT JOIN customers c ON o.cust_id = c.id";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Join { join_type, .. } = &result {
            assert_eq!(*join_type, JoinType::RightOuter);
        } else {
            panic!("expected Join at top level");
        }
    }

    #[test]
    fn test_full_outer_join() {
        let sql = "SELECT * FROM a \
                   FULL OUTER JOIN b ON a.id = b.id";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Join { join_type, .. } = &result {
            assert_eq!(*join_type, JoinType::FullOuter);
        } else {
            panic!("expected Join at top level");
        }
    }

    #[test]
    fn test_cross_join() {
        let sql = "SELECT * FROM a CROSS JOIN b";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Join { join_type, .. } = &result {
            assert_eq!(*join_type, JoinType::Cross);
        } else {
            panic!("expected Join at top level");
        }
    }

    // ---- Window function tests ----

    #[test]
    fn test_row_number_window() {
        let sql = "SELECT id, ROW_NUMBER() OVER (ORDER BY id) as rn \
                   FROM users";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn has_window(r: &RelExpr) -> bool {
            match r {
                RelExpr::Window { .. } => true,
                _ => r.children().iter().any(|c| has_window(c)),
            }
        }
        assert!(has_window(&result), "expected Window node");
    }

    #[test]
    fn test_rank_window_with_partition() {
        let sql = "SELECT dept, salary, \
                   RANK() OVER (PARTITION BY dept ORDER BY salary DESC) as rnk \
                   FROM employees";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn find_window(r: &RelExpr) -> Option<&RelExpr> {
            match r {
                RelExpr::Window { .. } => Some(r),
                _ => r
                    .children()
                    .into_iter()
                    .find_map(find_window),
            }
        }
        let win = find_window(&result)
            .expect("expected Window node");
        if let RelExpr::Window { functions, .. } = win {
            assert_eq!(functions.len(), 1);
            assert_eq!(
                functions[0].function,
                WindowFunction::Rank
            );
            assert_eq!(functions[0].partition_by.len(), 1);
            assert_eq!(functions[0].order_by.len(), 1);
        }
    }

    #[test]
    fn test_window_sum() {
        let sql = "SELECT id, \
                   SUM(amount) OVER (ORDER BY id) as running_total \
                   FROM orders";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn find_window(r: &RelExpr) -> Option<&RelExpr> {
            match r {
                RelExpr::Window { .. } => Some(r),
                _ => r
                    .children()
                    .into_iter()
                    .find_map(find_window),
            }
        }
        let win = find_window(&result)
            .expect("expected Window node");
        if let RelExpr::Window { functions, .. } = win {
            assert_eq!(functions[0].function, WindowFunction::Sum);
        }
    }

    // ---- Set operation tests ----

    #[test]
    fn test_union() {
        let sql = "SELECT id FROM a UNION SELECT id FROM b";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(matches!(result, RelExpr::Union { all: false, .. }));
    }

    #[test]
    fn test_union_all() {
        let sql = "SELECT id FROM a UNION ALL SELECT id FROM b";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(matches!(result, RelExpr::Union { all: true, .. }));
    }

    #[test]
    fn test_intersect() {
        let sql = "SELECT id FROM a INTERSECT SELECT id FROM b";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(matches!(
            result,
            RelExpr::Intersect { all: false, .. }
        ));
    }

    #[test]
    fn test_except() {
        let sql = "SELECT id FROM a EXCEPT SELECT id FROM b";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(matches!(
            result,
            RelExpr::Except { all: false, .. }
        ));
    }

    // ---- Extended aggregate tests ----

    #[test]
    fn test_stddev_aggregate() {
        let sql = "SELECT STDDEV(salary) FROM employees";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn find_agg(r: &RelExpr) -> Option<&RelExpr> {
            match r {
                RelExpr::Aggregate { .. } => Some(r),
                _ => r.children().into_iter().find_map(find_agg),
            }
        }
        let agg_node =
            find_agg(&result).expect("expected Aggregate");
        if let RelExpr::Aggregate { aggregates, .. } = agg_node {
            assert_eq!(
                aggregates[0].function,
                AggregateFunction::StdDev
            );
        }
    }

    #[test]
    fn test_variance_aggregate() {
        let sql = "SELECT VARIANCE(score) FROM tests";
        let result = sql_to_relexpr(sql).expect("should parse");
        fn find_agg(r: &RelExpr) -> Option<&RelExpr> {
            match r {
                RelExpr::Aggregate { .. } => Some(r),
                _ => r.children().into_iter().find_map(find_agg),
            }
        }
        let agg_node =
            find_agg(&result).expect("expected Aggregate");
        if let RelExpr::Aggregate { aggregates, .. } = agg_node {
            assert_eq!(
                aggregates[0].function,
                AggregateFunction::Variance
            );
        }
    }

    // ---- BETWEEN test ----

    #[test]
    fn test_between() {
        let sql =
            "SELECT * FROM orders WHERE amount BETWEEN 10 AND 100";
        let result = sql_to_relexpr(sql).expect("should parse");
        if let RelExpr::Filter { predicate, .. } = &result {
            assert!(matches!(predicate, Expr::BinOp { op: BinOp::And, .. }));
        } else {
            panic!("expected Filter at top level");
        }
    }

    // ---- CAST test ----

    #[test]
    fn test_cast() {
        let sql =
            "SELECT CAST(price AS INTEGER) FROM products";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok(), "CAST should parse");
    }

    // ---- CASE expression test ----

    #[test]
    fn test_case_expression() {
        let sql = "SELECT CASE WHEN age > 18 THEN 'adult' \
                   ELSE 'minor' END FROM users";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok(), "CASE should parse");
    }

    // ---- Combination tests ----

    #[test]
    fn test_cte_with_window() {
        let sql = "WITH ranked AS (\
                     SELECT id, \
                       ROW_NUMBER() OVER (ORDER BY id) as rn \
                     FROM users\
                   ) \
                   SELECT * FROM ranked WHERE rn <= 10";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok(), "CTE + window should parse");
    }

    #[test]
    fn test_distinct_with_order_by() {
        let sql = "SELECT DISTINCT name FROM users ORDER BY name";
        let result = sql_to_relexpr(sql).expect("should parse");
        // Should be Sort(Distinct(...))
        if let RelExpr::Sort { input, .. } = &result {
            assert!(matches!(
                input.as_ref(),
                RelExpr::Distinct { .. }
            ));
        } else {
            panic!("expected Sort(Distinct(...))");
        }
    }

    #[test]
    fn test_having_with_limit() {
        let sql = "SELECT dept_id, COUNT(*) as cnt \
                   FROM employees \
                   GROUP BY dept_id \
                   HAVING COUNT(*) > 5 \
                   LIMIT 10";
        let result = sql_to_relexpr(sql).expect("should parse");
        assert!(
            matches!(result, RelExpr::Limit { .. }),
            "expected Limit at top"
        );
    }

    #[test]
    fn test_complex_query() {
        let sql = "WITH dept_stats AS (\
                     SELECT dept_id, AVG(salary) as avg_sal \
                     FROM employees \
                     GROUP BY dept_id \
                     HAVING AVG(salary) > 50000\
                   ) \
                   SELECT DISTINCT d.dept_id \
                   FROM dept_stats d \
                   ORDER BY d.dept_id \
                   LIMIT 20 OFFSET 5";
        let result = sql_to_relexpr(sql);
        assert!(
            result.is_ok(),
            "complex query should parse: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_multiple_from_items() {
        let sql = "SELECT * FROM a, b WHERE a.id = b.id";
        let result = sql_to_relexpr(sql).expect("should parse");
        // Should create implicit cross join then filter
        fn has_join(r: &RelExpr) -> bool {
            match r {
                RelExpr::Join { .. } => true,
                _ => r.children().iter().any(|c| has_join(c)),
            }
        }
        assert!(has_join(&result), "expected implicit cross join");
    }

    #[test]
    fn test_join_using() {
        let sql =
            "SELECT * FROM orders JOIN customers USING (customer_id)";
        let result = sql_to_relexpr(sql);
        assert!(result.is_ok(), "JOIN USING should parse");
    }
}
