//! `#[no_mangle] extern "C"` builder functions called by the Lime parser.
//!
//! Every function here takes `*mut RaParseState` as its first argument and
//! returns `*mut RaNode`. If the state pointer is null, the function returns
//! null. If a child pointer cannot be decoded, the function records an error
//! on the state and returns null.
//!
//! The Lime grammar's reduction actions call these functions to build up the
//! `RelExpr` / `Expr` AST in the arenas managed by `RaParseState`.

use std::ffi::CStr;
use std::os::raw::c_char;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey, WindowExpr,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr};

use super::node::{decode, NodeTag, RaNode, RaParseState};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Safely dereference a `*mut RaParseState`, returning `None` (and thus null
/// from the caller) when the pointer is null.
///
/// # Safety
/// The pointer must be either null or point to a valid `RaParseState`.
unsafe fn state_ref<'a>(
    state: *mut RaParseState,
) -> Option<&'a mut RaParseState> {
    if state.is_null() {
        return None;
    }
    // SAFETY: caller guarantees non-null points to valid state
    Some(unsafe { &mut *state })
}

/// Convert a C string pointer to a Rust `String`.
///
/// Returns an empty string if the pointer is null or not valid UTF-8.
///
/// # Safety
/// The pointer must be either null or point to a valid NUL-terminated string.
unsafe fn c_str_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: caller guarantees the pointer is valid C string
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .unwrap_or("")
        .to_owned()
}

/// Decode a tagged pointer as a `RelExpr`, cloning it out of the arena.
fn decode_rel(
    state: &RaParseState,
    ptr: *mut RaNode,
) -> Option<RelExpr> {
    let (tag, idx) = decode(ptr)?;
    if tag != NodeTag::Rel {
        return None;
    }
    state.take_rel(idx)
}

/// Decode a tagged pointer as an `Expr`, cloning it out of the arena.
fn decode_expr(
    state: &RaParseState,
    ptr: *mut RaNode,
) -> Option<Expr> {
    let (tag, idx) = decode(ptr)?;
    if tag != NodeTag::Expr {
        return None;
    }
    state.take_expr(idx)
}

/// Decode a tagged pointer as a list of arena indices.
fn decode_list(
    state: &RaParseState,
    ptr: *mut RaNode,
) -> Option<Vec<usize>> {
    let (tag, idx) = decode(ptr)?;
    if tag != NodeTag::List {
        return None;
    }
    state.get_list(idx).map(<[usize]>::to_vec)
}

// ---------------------------------------------------------------------------
// Relational builders
// ---------------------------------------------------------------------------

/// Build a `Scan` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `table` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ra_scan(
    state: *mut RaParseState,
    table: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };
    st.push_rel(RelExpr::Scan {
        table: table_name,
        alias: None,
    })
}

/// Build a `Filter` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` and `predicate` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_filter(
    state: *mut RaParseState,
    input: *mut RaNode,
    predicate: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_filter: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(pred_expr) = decode_expr(st, predicate) else {
        st.push_error("ra_filter: invalid predicate node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Filter {
        predicate: pred_expr,
        input: Box::new(input_rel),
    })
}

/// Build a `Project` node.
///
/// `columns_list` is a tagged list pointer whose elements are `Expr` indices.
/// Each expression becomes a `ProjectionColumn` with no alias.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` and `columns_list` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_project(
    state: *mut RaParseState,
    input: *mut RaNode,
    columns_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_project: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(indices) = decode_list(st, columns_list) else {
        st.push_error("ra_project: invalid columns list".to_owned());
        return std::ptr::null_mut();
    };
    let mut columns = Vec::with_capacity(indices.len());
    for idx in indices {
        let Some(expr) = st.take_expr(idx) else {
            st.push_error(format!(
                "ra_project: invalid expr index {idx}"
            ));
            return std::ptr::null_mut();
        };
        columns.push(ProjectionColumn { expr, alias: None });
    }
    st.push_rel(RelExpr::Project {
        columns,
        input: Box::new(input_rel),
    })
}

/// Build a `Join` node.
///
/// `join_type` encoding: 0=Inner, 1=LeftOuter, 2=RightOuter, 3=FullOuter,
/// 4=Cross, 5=Semi, 6=Anti.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `left`, `right`, `condition` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_join(
    state: *mut RaParseState,
    join_type: u32,
    left: *mut RaNode,
    right: *mut RaNode,
    condition: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let jt = match join_type {
        0 => JoinType::Inner,
        1 => JoinType::LeftOuter,
        2 => JoinType::RightOuter,
        3 => JoinType::FullOuter,
        4 => JoinType::Cross,
        5 => JoinType::Semi,
        6 => JoinType::Anti,
        other => {
            st.push_error(format!(
                "ra_join: unknown join type {other}"
            ));
            return std::ptr::null_mut();
        }
    };
    let Some(left_rel) = decode_rel(st, left) else {
        st.push_error("ra_join: invalid left node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(right_rel) = decode_rel(st, right) else {
        st.push_error("ra_join: invalid right node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(cond_expr) = decode_expr(st, condition) else {
        st.push_error("ra_join: invalid condition node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Join {
        join_type: jt,
        condition: cond_expr,
        left: Box::new(left_rel),
        right: Box::new(right_rel),
    })
}

/// Build an `Aggregate` node.
///
/// `group_by_list` is a tagged list of `Expr` indices.
/// `agg_list` is a tagged list of `Expr` indices (each expr is pushed as an
/// `AggregateExpr` with `Count` function — callers should use `ra_agg_expr`
/// for full control).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - All pointer args must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_aggregate(
    state: *mut RaParseState,
    input: *mut RaNode,
    group_by_list: *mut RaNode,
    agg_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_aggregate: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    let group_by = collect_exprs(st, group_by_list);
    let agg_exprs = collect_exprs(st, agg_list);
    let aggregates: Vec<AggregateExpr> = agg_exprs
        .into_iter()
        .map(|expr| AggregateExpr {
            function: AggregateFunction::Count,
            arg: Some(expr),
            distinct: false,
            alias: None,
        })
        .collect();
    st.push_rel(RelExpr::Aggregate {
        group_by,
        aggregates,
        input: Box::new(input_rel),
    })
}

/// Build a `Sort` node.
///
/// `keys_list` is a tagged list of `SortKey` indices.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` and `keys_list` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_sort(
    state: *mut RaParseState,
    input: *mut RaNode,
    keys_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_sort: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(indices) = decode_list(st, keys_list) else {
        st.push_error("ra_sort: invalid keys list".to_owned());
        return std::ptr::null_mut();
    };
    let mut keys = Vec::with_capacity(indices.len());
    for idx in indices {
        let Some(key) = st.take_sort_key(idx) else {
            st.push_error(format!(
                "ra_sort: invalid sort key index {idx}"
            ));
            return std::ptr::null_mut();
        };
        keys.push(key);
    }
    st.push_rel(RelExpr::Sort {
        keys,
        input: Box::new(input_rel),
    })
}

/// Build a `Limit` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_limit(
    state: *mut RaParseState,
    input: *mut RaNode,
    count: u64,
    offset: u64,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_limit: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Limit {
        count,
        offset,
        input: Box::new(input_rel),
    })
}

/// Build a `Union` node.
///
/// `all`: 0 = UNION (dedup), nonzero = UNION ALL.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `left` and `right` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_union(
    state: *mut RaParseState,
    left: *mut RaNode,
    right: *mut RaNode,
    all: u32,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(left_rel) = decode_rel(st, left) else {
        st.push_error("ra_union: invalid left node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(right_rel) = decode_rel(st, right) else {
        st.push_error("ra_union: invalid right node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Union {
        all: all != 0,
        left: Box::new(left_rel),
        right: Box::new(right_rel),
    })
}

/// Build a `CTE` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be null or a valid NUL-terminated C string.
/// - `definition` and `body` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_cte(
    state: *mut RaParseState,
    name: *const c_char,
    definition: *mut RaNode,
    body: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let cte_name = unsafe { c_str_to_string(name) };
    let Some(def_rel) = decode_rel(st, definition) else {
        st.push_error("ra_cte: invalid definition node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(body_rel) = decode_rel(st, body) else {
        st.push_error("ra_cte: invalid body node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::CTE {
        name: cte_name,
        definition: Box::new(def_rel),
        body: Box::new(body_rel),
    })
}

/// Build a `Window` node.
///
/// `functions_list` is a tagged list; for now each entry is an `Expr` index
/// that becomes a `WindowExpr` with `RowNumber` function and no partitioning.
/// Use `ra_window_expr` for full control.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` and `functions_list` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_window(
    state: *mut RaParseState,
    input: *mut RaNode,
    functions_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_window: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    let exprs = collect_exprs(st, functions_list);
    let functions: Vec<WindowExpr> = exprs
        .into_iter()
        .map(|expr| WindowExpr {
            function: WindowFunction::RowNumber,
            arg: Some(expr),
            partition_by: vec![],
            order_by: vec![],
            frame: None,
            alias: None,
        })
        .collect();
    st.push_rel(RelExpr::Window {
        functions,
        input: Box::new(input_rel),
    })
}

/// Build a `Distinct` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `input` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_distinct(
    state: *mut RaParseState,
    input: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(input_rel) = decode_rel(st, input) else {
        st.push_error("ra_distinct: invalid input node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Distinct {
        input: Box::new(input_rel),
    })
}

// ---------------------------------------------------------------------------
// Expression builders
// ---------------------------------------------------------------------------

/// Build an unqualified `Column` expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ra_column(
    state: *mut RaParseState,
    name: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let col_name = unsafe { c_str_to_string(name) };
    st.push_expr(Expr::Column(ColumnRef::new(col_name)))
}

/// Build a table-qualified `Column` expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `table` and `column` must be null or valid NUL-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn ra_qualified_column(
    state: *mut RaParseState,
    table: *const c_char,
    column: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let tbl = unsafe { c_str_to_string(table) };
    let col = unsafe { c_str_to_string(column) };
    st.push_expr(Expr::Column(ColumnRef::qualified(tbl, col)))
}

/// Build an integer constant expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_const_int(
    state: *mut RaParseState,
    value: i64,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::Const(Const::Int(value)))
}

/// Build a float constant expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_const_float(
    state: *mut RaParseState,
    value: f64,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::Const(Const::Float(value)))
}

/// Build a string constant expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `value` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ra_const_str(
    state: *mut RaParseState,
    value: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let s = unsafe { c_str_to_string(value) };
    st.push_expr(Expr::Const(Const::String(s)))
}

/// Build a NULL constant expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_const_null(
    state: *mut RaParseState,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::Const(Const::Null))
}

/// Build a binary operation expression.
///
/// `op` encoding: 0=Add, 1=Sub, 2=Mul, 3=Div, 4=Eq, 5=Ne, 6=Lt, 7=Le,
/// 8=Gt, 9=Ge, 10=And, 11=Or, 12=Mod, 13=Concat, 14=JsonAccess.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `left` and `right` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_binop(
    state: *mut RaParseState,
    op: u32,
    left: *mut RaNode,
    right: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let bin_op = match op {
        0 => BinOp::Add,
        1 => BinOp::Sub,
        2 => BinOp::Mul,
        3 => BinOp::Div,
        4 => BinOp::Eq,
        5 => BinOp::Ne,
        6 => BinOp::Lt,
        7 => BinOp::Le,
        8 => BinOp::Gt,
        9 => BinOp::Ge,
        10 => BinOp::And,
        11 => BinOp::Or,
        12 => BinOp::Mod,
        13 => BinOp::Concat,
        14 => BinOp::JsonAccess,
        other => {
            st.push_error(format!(
                "ra_binop: unknown operator {other}"
            ));
            return std::ptr::null_mut();
        }
    };
    let Some(left_expr) = decode_expr(st, left) else {
        st.push_error("ra_binop: invalid left operand".to_owned());
        return std::ptr::null_mut();
    };
    let Some(right_expr) = decode_expr(st, right) else {
        st.push_error("ra_binop: invalid right operand".to_owned());
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::BinOp {
        op: bin_op,
        left: Box::new(left_expr),
        right: Box::new(right_expr),
    })
}

/// Build a function call expression.
///
/// `args_list` is a tagged list of `Expr` indices.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be null or a valid NUL-terminated C string.
/// - `args_list` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_func(
    state: *mut RaParseState,
    name: *const c_char,
    args_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let func_name = unsafe { c_str_to_string(name) };
    let args = collect_exprs(st, args_list);
    st.push_expr(Expr::Function {
        name: func_name,
        args,
    })
}

// ---------------------------------------------------------------------------
// List builders
// ---------------------------------------------------------------------------

/// Create a new empty list.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_list_new(
    state: *mut RaParseState,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_list()
}

/// Append an item to a list. Returns the list pointer unchanged on success,
/// or null on failure.
///
/// The `item` pointer is decoded and its arena index is stored in the list.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `list` and `item` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_list_push(
    state: *mut RaParseState,
    list: *mut RaNode,
    item: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some((NodeTag::List, list_idx)) = decode(list) else {
        st.push_error("ra_list_push: invalid list pointer".to_owned());
        return std::ptr::null_mut();
    };
    let Some((_tag, item_idx)) = decode(item) else {
        st.push_error("ra_list_push: invalid item pointer".to_owned());
        return std::ptr::null_mut();
    };
    if st.list_push(list_idx, item_idx) {
        list
    } else {
        st.push_error(format!(
            "ra_list_push: list index {list_idx} out of bounds"
        ));
        std::ptr::null_mut()
    }
}

/// Build a `SortKey` node.
///
/// `ascending`: 0 = DESC, nonzero = ASC.
/// `nulls_first`: 0 = NULLS LAST, nonzero = NULLS FIRST.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `expr` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_sort_key(
    state: *mut RaParseState,
    expr: *mut RaNode,
    ascending: u32,
    nulls_first: u32,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(key_expr) = decode_expr(st, expr) else {
        st.push_error(
            "ra_sort_key: invalid expression node".to_owned(),
        );
        return std::ptr::null_mut();
    };
    let direction = if ascending != 0 {
        SortDirection::Asc
    } else {
        SortDirection::Desc
    };
    let nulls = if nulls_first != 0 {
        NullOrdering::First
    } else {
        NullOrdering::Last
    };
    st.push_sort_key(SortKey {
        expr: key_expr,
        direction,
        nulls,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Collect `Expr` values from a tagged list pointer.
///
/// Returns an empty vec if the pointer is null or not a valid list.
fn collect_exprs(
    state: &RaParseState,
    list_ptr: *mut RaNode,
) -> Vec<Expr> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut result = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(expr) = state.take_expr(idx) {
            result.push(expr);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a state and return a raw pointer to it.
    fn new_state() -> *mut RaParseState {
        Box::into_raw(Box::new(RaParseState::new()))
    }

    /// Helper: reclaim the state box.
    ///
    /// # Safety
    /// The pointer must have come from `new_state`.
    unsafe fn free_state(state: *mut RaParseState) -> RaParseState {
        unsafe { *Box::from_raw(state) }
    }

    #[test]
    fn null_state_returns_null() {
        let result =
            unsafe { ra_scan(std::ptr::null_mut(), std::ptr::null()) };
        assert!(result.is_null());
    }

    #[test]
    fn scan_produces_valid_node() {
        let st = new_state();
        let table = c"users";
        let node = unsafe { ra_scan(st, table.as_ptr()) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(
            matches!(rel, RelExpr::Scan { table, .. } if table == "users")
        );
    }

    #[test]
    fn filter_builds_correctly() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"orders".as_ptr()) };
        let lhs = unsafe { ra_column(st, c"amount".as_ptr()) };
        let rhs = unsafe { ra_const_int(st, 100) };
        let pred = unsafe { ra_binop(st, 8, lhs, rhs) }; // Gt
        let filter = unsafe { ra_filter(st, scan, pred) };
        assert!(!filter.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Filter { .. }));
    }

    #[test]
    fn join_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let cond = unsafe { ra_const_int(st, 1) };
        // 1 for Expr -> we need a bool-ish constant for the condition
        let join = unsafe { ra_join(st, 0, left, right, cond) };
        assert!(!join.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Join { join_type: JoinType::Inner, .. }));
    }

    #[test]
    fn limit_builds_correctly() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let limited = unsafe { ra_limit(st, scan, 10, 5) };
        assert!(!limited.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(
            matches!(rel, RelExpr::Limit { count: 10, offset: 5, .. })
        );
    }

    #[test]
    fn union_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let union_node = unsafe { ra_union(st, left, right, 1) };
        assert!(!union_node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(
            matches!(rel, RelExpr::Union { all: true, .. })
        );
    }

    #[test]
    fn distinct_builds_correctly() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let dist = unsafe { ra_distinct(st, scan) };
        assert!(!dist.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Distinct { .. }));
    }

    #[test]
    fn const_null_builds_correctly() {
        let st = new_state();
        let node = unsafe { ra_const_null(st) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Const(Const::Null));
    }

    #[test]
    fn const_float_builds_correctly() {
        let st = new_state();
        let node = unsafe { ra_const_float(st, 3.14) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(
            matches!(expr, Expr::Const(Const::Float(v)) if (v - 3.14).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn const_str_builds_correctly() {
        let st = new_state();
        let node = unsafe { ra_const_str(st, c"hello".as_ptr()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Const(Const::String("hello".to_owned())));
    }

    #[test]
    fn qualified_column_builds_correctly() {
        let st = new_state();
        let node = unsafe {
            ra_qualified_column(st, c"users".as_ptr(), c"id".as_ptr())
        };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(
            expr,
            Expr::Column(ColumnRef::qualified("users", "id"))
        );
    }

    #[test]
    fn list_and_project() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let col = unsafe { ra_column(st, c"x".as_ptr()) };
        let list = unsafe { ra_list_new(st) };
        let list = unsafe { ra_list_push(st, list, col) };
        assert!(!list.is_null());
        let proj = unsafe { ra_project(st, scan, list) };
        assert!(!proj.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Project { .. }));
    }

    #[test]
    fn sort_key_and_sort() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let col = unsafe { ra_column(st, c"id".as_ptr()) };
        let sk = unsafe { ra_sort_key(st, col, 1, 0) };
        assert!(!sk.is_null());
        let list = unsafe { ra_list_new(st) };
        let list = unsafe { ra_list_push(st, list, sk) };
        let sorted = unsafe { ra_sort(st, scan, list) };
        assert!(!sorted.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        if let RelExpr::Sort { keys, .. } = &rel {
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].direction, SortDirection::Asc);
            assert_eq!(keys[0].nulls, NullOrdering::Last);
        } else {
            panic!("expected Sort variant");
        }
    }

    #[test]
    fn func_builds_correctly() {
        let st = new_state();
        let arg = unsafe { ra_column(st, c"x".as_ptr()) };
        let list = unsafe { ra_list_new(st) };
        let list = unsafe { ra_list_push(st, list, arg) };
        let func = unsafe { ra_func(st, c"UPPER".as_ptr(), list) };
        assert!(!func.is_null());

        let (tag, idx) = decode(func).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(
            matches!(expr, Expr::Function { name, args } if name == "UPPER" && args.len() == 1)
        );
    }

    #[test]
    fn cte_builds_correctly() {
        let st = new_state();
        let def = unsafe { ra_scan(st, c"source".as_ptr()) };
        let body = unsafe { ra_scan(st, c"temp".as_ptr()) };
        let cte = unsafe { ra_cte(st, c"temp".as_ptr(), def, body) };
        assert!(!cte.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(
            matches!(rel, RelExpr::CTE { name, .. } if name == "temp")
        );
    }

    #[test]
    fn invalid_join_type_records_error() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let cond = unsafe { ra_const_int(st, 1) };
        let join = unsafe { ra_join(st, 99, left, right, cond) };
        assert!(join.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs[0].contains("unknown join type"));
    }

    #[test]
    fn invalid_binop_records_error() {
        let st = new_state();
        let lhs = unsafe { ra_const_int(st, 1) };
        let rhs = unsafe { ra_const_int(st, 2) };
        let result = unsafe { ra_binop(st, 99, lhs, rhs) };
        assert!(result.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs[0].contains("unknown operator"));
    }
}
