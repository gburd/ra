use egg::Id;
use ra_core::expr::Const;

use crate::egraph::{EGraphError, RelLang};

use super::helpers::{convert_bool_flag, get_symbol, id};

pub(crate) fn convert_scalar(
    nodes: &[RelLang],
    idx: usize,
) -> Result<ra_core::expr::Expr, EGraphError> {
    use ra_core::expr::{ColumnRef, Const, Expr};

    match &nodes[idx] {
        RelLang::Col([name_id]) => {
            let name = get_symbol(nodes, id(*name_id))?;
            Ok(Expr::Column(ColumnRef::new(name)))
        }
        RelLang::QCol([table_id, name_id]) => {
            let table = get_symbol(nodes, id(*table_id))?;
            let name = get_symbol(nodes, id(*name_id))?;
            Ok(Expr::Column(ColumnRef::qualified(table, name)))
        }
        RelLang::ConstNull => Ok(Expr::Const(Const::Null)),
        RelLang::ConstBool([v]) => convert_const_bool(nodes, *v),
        RelLang::ConstInt([v]) => convert_const_int(nodes, *v),
        RelLang::ConstFloat([v]) => convert_const_float(nodes, *v),
        RelLang::ConstStr([v]) => Ok(Expr::Const(Const::String(get_symbol(nodes, id(*v))?))),
        node => convert_scalar_operator(nodes, idx, node),
    }
}

fn convert_const_bool(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let b = convert_bool_flag(nodes, id(val_id))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Bool(b)))
}

fn convert_const_int(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let s = get_symbol(nodes, id(val_id))?;
    let i = s
        .parse::<i64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid int: {e}")))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Int(i)))
}

fn convert_const_float(nodes: &[RelLang], val_id: Id) -> Result<ra_core::expr::Expr, EGraphError> {
    let s = get_symbol(nodes, id(val_id))?;
    let f = s
        .parse::<f64>()
        .map_err(|e| EGraphError::ExtractionError(format!("invalid float: {e}")))?;
    Ok(ra_core::expr::Expr::Const(ra_core::expr::Const::Float(f)))
}

#[expect(clippy::too_many_lines, reason = "match over all scalar operator variants")]
fn convert_scalar_operator(
    nodes: &[RelLang],
    idx: usize,
    node: &RelLang,
) -> Result<ra_core::expr::Expr, EGraphError> {
    use ra_core::expr::Expr;

    if let Some((op, l, r)) = try_binop(node) {
        return make_binop(nodes, op, id(l), id(r));
    }
    if let Some((op, operand)) = try_unaryop(node) {
        let inner = convert_scalar(nodes, id(operand))?;
        return Ok(Expr::UnaryOp {
            op,
            operand: Box::new(inner),
        });
    }
    // Handle Cast operator
    if let RelLang::Cast([expr_id, type_id]) = node {
        let expr = convert_scalar(nodes, id(*expr_id))?;
        let target_type = get_symbol(nodes, id(*type_id))?;
        return Ok(Expr::Cast {
            expr: Box::new(expr),
            target_type,
        });
    }

    // Handle Vector/FTS operators
    if let RelLang::VectorDistance([metric_id, col_id, target_id]) = node {
        let metric = get_symbol(nodes, id(*metric_id))?;
        let column = convert_scalar(nodes, id(*col_id))?;
        let target = convert_scalar(nodes, id(*target_id))?;
        return Ok(Expr::VectorDistance {
            metric,
            column: Box::new(column),
            target: Box::new(target),
        });
    }
    if let RelLang::FtsMatch([vendor_id, cols_id, query_id, mode_id]) = node {
        let vendor = get_symbol(nodes, id(*vendor_id))?;
        let cols_str = get_symbol(nodes, id(*cols_id))?;
        let columns = cols_str
            .split(',')
            .map(std::string::ToString::to_string)
            .collect();
        let query = get_symbol(nodes, id(*query_id))?;
        let mode_str = get_symbol(nodes, id(*mode_id))?;
        let mode = if mode_str.is_empty() {
            None
        } else {
            Some(mode_str)
        };
        return Ok(Expr::FullTextMatch {
            vendor,
            columns,
            query,
            mode,
        });
    }
    if let RelLang::FtsRank([col_id, query_id, algo_id]) = node {
        let col = convert_scalar(nodes, id(*col_id))?;
        let query = convert_scalar(nodes, id(*query_id))?;
        let algo = get_symbol(nodes, id(*algo_id))?;
        // FTS rank is represented as a function call in Expr
        return Ok(Expr::Function {
            name: format!("ts_rank_{algo}"),
            args: vec![col, query],
        });
    }

    if let RelLang::Func(ids) = node {
        if ids.is_empty() {
            return Err(EGraphError::ExtractionError("empty function call".into()));
        }
        let name = get_symbol(nodes, id(ids[0]))?;
        // Reconstruct Array and ArrayIndex from their tagged Func encoding
        if name == "ARRAY" {
            let mut elements = Vec::with_capacity(ids.len() - 1);
            for &fid in &ids[1..] {
                elements.push(convert_scalar(nodes, id(fid))?);
            }
            return Ok(Expr::Array(elements));
        }
        if name == "ARRAY_INDEX" && ids.len() == 3 {
            let array = convert_scalar(nodes, id(ids[1]))?;
            let index = convert_scalar(nodes, id(ids[2]))?;
            return Ok(Expr::ArrayIndex(Box::new(array), Box::new(index)));
        }
        if name == "ARRAY_SLICE" && ids.len() == 4 {
            let array = convert_scalar(nodes, id(ids[1]))?;
            let start_expr = convert_scalar(nodes, id(ids[2]))?;
            let end_expr = convert_scalar(nodes, id(ids[3]))?;
            let start = if start_expr == Expr::Const(Const::Null) {
                None
            } else {
                Some(Box::new(start_expr))
            };
            let end = if end_expr == Expr::Const(Const::Null) {
                None
            } else {
                Some(Box::new(end_expr))
            };
            return Ok(Expr::ArraySlice {
                array: Box::new(array),
                start,
                end,
            });
        }
        let mut args = Vec::with_capacity(ids.len() - 1);
        for &fid in &ids[1..] {
            args.push(convert_scalar(nodes, id(fid))?);
        }
        return Ok(Expr::Function { name, args });
    }
    Err(EGraphError::ExtractionError(format!(
        "unexpected scalar node at index {idx}: {node:?}"
    )))
}

fn try_binop(node: &RelLang) -> Option<(ra_core::expr::BinOp, Id, Id)> {
    use ra_core::expr::BinOp;
    let (op, l, r) = match node {
        RelLang::Add([l, r]) => (BinOp::Add, *l, *r),
        RelLang::Sub([l, r]) => (BinOp::Sub, *l, *r),
        RelLang::Mul([l, r]) => (BinOp::Mul, *l, *r),
        RelLang::Div([l, r]) => (BinOp::Div, *l, *r),
        RelLang::Mod([l, r]) => (BinOp::Mod, *l, *r),
        RelLang::Eq([l, r]) => (BinOp::Eq, *l, *r),
        RelLang::Ne([l, r]) => (BinOp::Ne, *l, *r),
        RelLang::Lt([l, r]) => (BinOp::Lt, *l, *r),
        RelLang::Le([l, r]) => (BinOp::Le, *l, *r),
        RelLang::Gt([l, r]) => (BinOp::Gt, *l, *r),
        RelLang::Ge([l, r]) => (BinOp::Ge, *l, *r),
        RelLang::And([l, r]) => (BinOp::And, *l, *r),
        RelLang::Or([l, r]) => (BinOp::Or, *l, *r),
        RelLang::Concat([l, r]) => (BinOp::Concat, *l, *r),
        RelLang::JsonAccess([l, r]) => (BinOp::JsonAccess, *l, *r),
        _ => return None,
    };
    Some((op, l, r))
}

fn try_unaryop(node: &RelLang) -> Option<(ra_core::expr::UnaryOp, Id)> {
    use ra_core::expr::UnaryOp;
    let (op, operand) = match node {
        RelLang::Not([o]) => (UnaryOp::Not, *o),
        RelLang::IsNull([o]) => (UnaryOp::IsNull, *o),
        RelLang::IsNotNull([o]) => (UnaryOp::IsNotNull, *o),
        RelLang::Neg([o]) => (UnaryOp::Neg, *o),
        _ => return None,
    };
    Some((op, operand))
}

fn make_binop(
    nodes: &[RelLang],
    op: ra_core::expr::BinOp,
    left_idx: usize,
    right_idx: usize,
) -> Result<ra_core::expr::Expr, EGraphError> {
    let left = convert_scalar(nodes, left_idx)?;
    let right = convert_scalar(nodes, right_idx)?;
    Ok(ra_core::expr::Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}
