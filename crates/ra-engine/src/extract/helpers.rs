use egg::Id;

use crate::egraph::{EGraphError, RelLang};

use super::scalar::convert_scalar;

pub(crate) fn id(egg_id: Id) -> usize {
    usize::from(egg_id)
}

pub(crate) fn get_symbol(nodes: &[RelLang], idx: usize) -> Result<String, EGraphError> {
    if let RelLang::Symbol(s) = &nodes[idx] {
        Ok(s.to_string())
    } else {
        Err(EGraphError::ExtractionError(format!(
            "expected Symbol at index {idx}, got {:?}",
            nodes[idx]
        )))
    }
}

pub(crate) fn convert_bool_flag(nodes: &[RelLang], idx: usize) -> Result<bool, EGraphError> {
    match &nodes[idx] {
        RelLang::True => Ok(true),
        RelLang::False => Ok(false),
        other => Err(EGraphError::ExtractionError(format!(
            "expected True/False, got {other:?}"
        ))),
    }
}

pub(crate) fn convert_join_type(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::JoinType, EGraphError> {
    use ra_core::algebra::JoinType;
    match &nodes[idx] {
        RelLang::Inner => Ok(JoinType::Inner),
        RelLang::LeftOuter => Ok(JoinType::LeftOuter),
        RelLang::RightOuter => Ok(JoinType::RightOuter),
        RelLang::FullOuter => Ok(JoinType::FullOuter),
        RelLang::Cross => Ok(JoinType::Cross),
        RelLang::Semi => Ok(JoinType::Semi),
        RelLang::Anti => Ok(JoinType::Anti),
        other => Err(EGraphError::ExtractionError(format!(
            "expected join type, got {other:?}"
        ))),
    }
}

pub(crate) fn convert_projection_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::ProjectionColumn>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for projection columns".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_proj_col(nodes, id(child)))
        .collect()
}

fn convert_proj_col(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::ProjectionColumn, EGraphError> {
    match &nodes[idx] {
        RelLang::ProjCol([expr_id]) => Ok(ra_core::algebra::ProjectionColumn {
            expr: convert_scalar(nodes, id(*expr_id))?,
            alias: None,
        }),
        RelLang::ProjAlias([expr_id, alias_id]) => Ok(ra_core::algebra::ProjectionColumn {
            expr: convert_scalar(nodes, id(*expr_id))?,
            alias: Some(get_symbol(nodes, id(*alias_id))?),
        }),
        other => Err(EGraphError::ExtractionError(format!(
            "expected ProjCol/ProjAlias, got {other:?}"
        ))),
    }
}

pub(crate) fn convert_scalar_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::expr::Expr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for expression list".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_scalar(nodes, id(child)))
        .collect()
}

pub(crate) fn convert_aggregate_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::AggregateExpr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for aggregate list".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_agg_expr(nodes, id(child)))
        .collect()
}

fn convert_agg_expr(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::AggregateExpr, EGraphError> {
    let RelLang::AggExpr([func_id, distinct_id, alias_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected AggExpr, got {:?}",
            nodes[idx]
        )));
    };
    let (function, arg) = convert_agg_function(nodes, id(*func_id))?;
    let distinct = match &nodes[id(*distinct_id)] {
        RelLang::Distinct => true,
        RelLang::All => false,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Distinct/All, got {other:?}"
            )))
        }
    };
    let alias = match &nodes[id(*alias_id)] {
        RelLang::Nil => None,
        RelLang::Symbol(s) => Some(s.to_string()),
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Nil/Symbol for alias, got {other:?}"
            )))
        }
    };
    Ok(ra_core::algebra::AggregateExpr {
        function,
        arg,
        distinct,
        alias,
    })
}

fn convert_agg_function(
    nodes: &[RelLang],
    idx: usize,
) -> Result<
    (
        ra_core::algebra::AggregateFunction,
        Option<ra_core::expr::Expr>,
    ),
    EGraphError,
> {
    use ra_core::algebra::AggregateFunction;
    let (func, arg_id) = match &nodes[idx] {
        RelLang::Count([a]) => (AggregateFunction::Count, *a),
        RelLang::Sum([a]) => (AggregateFunction::Sum, *a),
        RelLang::Avg([a]) => (AggregateFunction::Avg, *a),
        RelLang::Min([a]) => (AggregateFunction::Min, *a),
        RelLang::Max([a]) => (AggregateFunction::Max, *a),
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected aggregate function, got {other:?}"
            )))
        }
    };
    let arg_idx = id(arg_id);
    let arg = match &nodes[arg_idx] {
        RelLang::Nil => None,
        _ => Some(convert_scalar(nodes, arg_idx)?),
    };
    Ok((func, arg))
}

pub(crate) fn convert_window_expr_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::WindowExpr>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for window expressions".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_window_expr(nodes, id(child)))
        .collect()
}

fn convert_window_expr(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowExpr, EGraphError> {
    let RelLang::WindowExprNode([fn_id, arg_id, part_id, order_id, frame_id, alias_id]) =
        &nodes[idx]
    else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowExprNode, got {:?}",
            nodes[idx]
        )));
    };
    let function = convert_window_fn(nodes, id(*fn_id))?;
    let arg = convert_optional_scalar(nodes, id(*arg_id))?;
    let partition_by = convert_scalar_list(nodes, id(*part_id))?;
    let order_by = convert_sort_key_list(nodes, id(*order_id))?;
    let frame = convert_window_frame(nodes, id(*frame_id))?;
    let alias = convert_optional_symbol(nodes, id(*alias_id))?;
    Ok(ra_core::algebra::WindowExpr {
        function,
        arg,
        partition_by,
        order_by,
        frame,
        alias,
    })
}

fn convert_window_fn(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowFunction, EGraphError> {
    use ra_core::algebra::WindowFunction;
    let RelLang::WindowFn([name_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowFn, got {:?}",
            nodes[idx]
        )));
    };
    let name = get_symbol(nodes, id(*name_id))?;
    match name.as_str() {
        "RowNumber" => Ok(WindowFunction::RowNumber),
        "Rank" => Ok(WindowFunction::Rank),
        "DenseRank" => Ok(WindowFunction::DenseRank),
        "PercentRank" => Ok(WindowFunction::PercentRank),
        "Ntile" => Ok(WindowFunction::Ntile),
        "Lag" => Ok(WindowFunction::Lag),
        "Lead" => Ok(WindowFunction::Lead),
        "FirstValue" => Ok(WindowFunction::FirstValue),
        "LastValue" => Ok(WindowFunction::LastValue),
        "NthValue" => Ok(WindowFunction::NthValue),
        "Avg" => Ok(WindowFunction::Avg),
        "Sum" => Ok(WindowFunction::Sum),
        "Count" => Ok(WindowFunction::Count),
        "Min" => Ok(WindowFunction::Min),
        "Max" => Ok(WindowFunction::Max),
        other => Err(EGraphError::ExtractionError(format!(
            "unknown window function: {other}"
        ))),
    }
}

fn convert_window_frame(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Option<ra_core::algebra::WindowFrame>, EGraphError> {
    use ra_core::algebra::{WindowFrame, WindowFrameMode};
    if let RelLang::Nil = &nodes[idx] {
        return Ok(None);
    }
    let RelLang::WindowFrameNode([mode_id, start_id, end_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected WindowFrameNode, got {:?}",
            nodes[idx]
        )));
    };
    let mode = match &nodes[id(*mode_id)] {
        RelLang::FrameRows => WindowFrameMode::Rows,
        RelLang::FrameRange => WindowFrameMode::Range,
        RelLang::FrameGroups => WindowFrameMode::Groups,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected frame mode, got {other:?}"
            )))
        }
    };
    let start = convert_frame_bound(nodes, id(*start_id))?;
    let end = convert_frame_bound(nodes, id(*end_id))?;
    Ok(Some(WindowFrame { mode, start, end }))
}

fn convert_frame_bound(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::WindowFrameBound, EGraphError> {
    use ra_core::algebra::WindowFrameBound;
    match &nodes[idx] {
        RelLang::FrameUnboundedPreceding => Ok(WindowFrameBound::UnboundedPreceding),
        RelLang::FramePreceding([n_id]) => {
            let s = get_symbol(nodes, id(*n_id))?;
            let n = s
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid frame bound: {e}")))?;
            Ok(WindowFrameBound::Preceding(n))
        }
        RelLang::FrameCurrentRow => Ok(WindowFrameBound::CurrentRow),
        RelLang::FrameFollowing([n_id]) => {
            let s = get_symbol(nodes, id(*n_id))?;
            let n = s
                .parse::<u64>()
                .map_err(|e| EGraphError::ExtractionError(format!("invalid frame bound: {e}")))?;
            Ok(WindowFrameBound::Following(n))
        }
        RelLang::FrameUnboundedFollowing => Ok(WindowFrameBound::UnboundedFollowing),
        other => Err(EGraphError::ExtractionError(format!(
            "expected frame bound, got {other:?}"
        ))),
    }
}

fn convert_optional_scalar(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Option<ra_core::expr::Expr>, EGraphError> {
    if let RelLang::Nil = &nodes[idx] {
        return Ok(None);
    }
    Ok(Some(convert_scalar(nodes, idx)?))
}

fn convert_optional_symbol(nodes: &[RelLang], idx: usize) -> Result<Option<String>, EGraphError> {
    match &nodes[idx] {
        RelLang::Nil => Ok(None),
        RelLang::Symbol(s) => Ok(Some(s.to_string())),
        other => Err(EGraphError::ExtractionError(format!(
            "expected Nil/Symbol, got {other:?}"
        ))),
    }
}

pub(crate) fn convert_sort_key_list(
    nodes: &[RelLang],
    idx: usize,
) -> Result<Vec<ra_core::algebra::SortKey>, EGraphError> {
    let RelLang::List(ids) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(
            "expected List for sort keys".into(),
        ));
    };
    ids.iter()
        .map(|&child| convert_sort_key(nodes, id(child)))
        .collect()
}

fn convert_sort_key(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::algebra::SortKey, EGraphError> {
    let RelLang::SortKey([expr_id, dir_id, nulls_id]) = &nodes[idx] else {
        return Err(EGraphError::ExtractionError(format!(
            "expected SortKey, got {:?}",
            nodes[idx]
        )));
    };
    let direction = match &nodes[id(*dir_id)] {
        RelLang::Asc => ra_core::algebra::SortDirection::Asc,
        RelLang::Desc => ra_core::algebra::SortDirection::Desc,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected Asc/Desc, got {other:?}"
            )))
        }
    };
    let nulls = match &nodes[id(*nulls_id)] {
        RelLang::NullsFirst => ra_core::algebra::NullOrdering::First,
        RelLang::NullsLast => ra_core::algebra::NullOrdering::Last,
        other => {
            return Err(EGraphError::ExtractionError(format!(
                "expected NullsFirst/NullsLast, got {other:?}"
            )))
        }
    };
    Ok(ra_core::algebra::SortKey {
        expr: convert_scalar(nodes, id(*expr_id))?,
        direction,
        nulls,
    })
}
