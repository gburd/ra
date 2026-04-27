use ra_core::algebra::{
    SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode, WindowFunction,
};
use ra_core::expr::Expr;
use sqlparser::ast::{
    SelectItem, WindowFrameBound as SqlWindowFrameBound, WindowFrameUnits as SqlWindowFrameUnits,
    WindowSpec as SqlWindowSpec, WindowType as SqlWindowType,
};

use super::error::SqlConversionError;
use super::expr::convert_expr;
use super::helpers::{extract_single_func_arg, extract_u64_from_expr};
use super::select::convert_order_by_exprs;

pub fn extract_window_functions(
    projection: &[SelectItem],
) -> Result<Vec<WindowExpr>, SqlConversionError> {
    let mut window_fns = Vec::new();

    for item in projection {
        let (expr, alias) = match item {
            SelectItem::UnnamedExpr(e) => (e, None),
            SelectItem::ExprWithAlias { expr, alias } => (expr, Some(alias.value.clone())),
            _ => continue,
        };

        if let sqlparser::ast::Expr::Function(func) = expr {
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
            return Err(SqlConversionError::UnsupportedFeature(format!(
                "window function {func_name} not supported"
            )))
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

    let (partition_by, order_by, frame) = convert_window_spec(spec)?;

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
) -> Result<(Vec<Expr>, Vec<SortKey>, Option<WindowFrame>), SqlConversionError> {
    let partition_by: Result<Vec<_>, _> = spec.partition_by.iter().map(convert_expr).collect();
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
        SqlWindowFrameBound::CurrentRow => Ok(WindowFrameBound::CurrentRow),
        SqlWindowFrameBound::Preceding(None) => Ok(WindowFrameBound::UnboundedPreceding),
        SqlWindowFrameBound::Preceding(Some(expr)) => {
            let n = extract_u64_from_expr(expr)?;
            Ok(WindowFrameBound::Preceding(n))
        }
        SqlWindowFrameBound::Following(None) => Ok(WindowFrameBound::UnboundedFollowing),
        SqlWindowFrameBound::Following(Some(expr)) => {
            let n = extract_u64_from_expr(expr)?;
            Ok(WindowFrameBound::Following(n))
        }
    }
}

pub fn is_window_function(expr: &sqlparser::ast::Expr) -> bool {
    matches!(
        expr,
        sqlparser::ast::Expr::Function(func) if func.over.is_some()
    )
}
