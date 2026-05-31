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
use std::os::raw::{c_char, c_int, c_void};

use crate::lime_parser::diagnostics;
use crate::lime_parser::lexer::RaToken;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, CycleDetection, EdgeDirection, GraphPatternElement, JoinType,
    MergeAction, MergeMatchKind, MergeWhen, NullOrdering, OnConflict, ProjectionColumn, RelExpr,
    SortDirection, SortKey, WindowExpr, WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, SubQueryType, UnaryOp};

use super::node::{decode, NodeTag, RaNode, RaParseState, StructuredParseError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Safely dereference a `*mut RaParseState`, returning `None` (and thus null
/// from the caller) when the pointer is null.
///
/// # Safety
/// The pointer must be either null or point to a valid `RaParseState`.
unsafe fn state_ref<'a>(state: *mut RaParseState) -> Option<&'a mut RaParseState> {
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

/// Convert a C string with explicit length to a Rust `String`.
///
/// # Safety
/// `ptr` must point to at least `len` valid UTF-8 bytes (no NUL required).
unsafe fn c_str_len_to_string(ptr: *const c_char, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: caller guarantees ptr points to at least len bytes
    let slice = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) };
    String::from_utf8_lossy(slice).into_owned()
}

/// Decode a tagged pointer as a `RelExpr`, cloning it out of the arena.
fn decode_rel(state: &RaParseState, ptr: *mut RaNode) -> Option<RelExpr> {
    let (tag, idx) = decode(ptr)?;
    if tag != NodeTag::Rel {
        return None;
    }
    state.take_rel(idx)
}

/// Decode a tagged pointer as an `Expr`, cloning it out of the arena.
fn decode_expr(state: &RaParseState, ptr: *mut RaNode) -> Option<Expr> {
    let (tag, idx) = decode(ptr)?;
    if tag != NodeTag::Expr {
        return None;
    }
    state.take_expr(idx)
}

/// Decode a tagged pointer as a list of arena indices.
fn decode_list(state: &RaParseState, ptr: *mut RaNode) -> Option<Vec<usize>> {
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
pub unsafe extern "C" fn ra_scan(state: *mut RaParseState, table: *const c_char) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };
    st.push_rel(RelExpr::Scan {
        table: table_name,
        alias: None,
    })
}

/// Build a `Scan` node with an alias.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `table` and `alias` must be null or valid NUL-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn ra_scan_alias(
    state: *mut RaParseState,
    table: *const c_char,
    alias: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };
    let alias_name = unsafe { c_str_to_string(alias) };
    st.push_rel(RelExpr::Scan {
        table: table_name,
        alias: Some(alias_name),
    })
}

/// Build a FILTER-clause aggregate: `agg(args) FILTER (WHERE cond)`.
///
/// Rewrites to `agg(CASE WHEN cond THEN first_arg ELSE NULL END)`.
/// If the argument list has one element, that element becomes the THEN
/// clause. Otherwise the first argument in the list is used.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `func_name` must be a valid NUL-terminated C string.
/// - `args_list` must be a valid list node, `filter_cond` a valid expr node.
#[no_mangle]
pub unsafe extern "C" fn ra_filter_agg(
    state: *mut RaParseState,
    func_name: *const c_char,
    args_list: *mut RaNode,
    filter_cond: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let name = unsafe { c_str_to_string(func_name) };

    // Decode the condition expression
    let Some(cond) = decode_expr(st, filter_cond) else {
        st.push_error("ra_filter_agg: invalid filter condition".to_owned());
        return std::ptr::null_mut();
    };

    // Decode the argument list and get the first argument
    let Some(list_indices) = decode_list(st, args_list) else {
        st.push_error("ra_filter_agg: invalid args list".to_owned());
        return std::ptr::null_mut();
    };

    let first_arg = if let Some(&idx) = list_indices.first() {
        st.take_expr(idx)
            .unwrap_or(Expr::Column(ColumnRef::new("*")))
    } else {
        Expr::Column(ColumnRef::new("*"))
    };

    // Build: CASE WHEN cond THEN first_arg ELSE NULL END
    let case_expr = Expr::Case {
        operand: None,
        when_clauses: vec![(cond, first_arg)],
        else_result: Some(Box::new(Expr::Const(Const::Null))),
    };

    // Build: func(case_expr)
    let func_expr = Expr::Function {
        name,
        args: vec![case_expr],
    };

    st.push_expr(func_expr)
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
            st.push_error(format!("ra_project: invalid expr index {idx}"));
            return std::ptr::null_mut();
        };
        columns.push(ProjectionColumn {
            expr,
            alias: st.expr_alias(idx),
        });
    }
    st.push_rel(RelExpr::Project {
        columns,
        input: Box::new(input_rel),
    })
}

/// Record an output-column alias for a SELECT-list item (`expr AS alias`)
/// and return the expr pointer unchanged, so the alias rides through
/// `target_list` to `ra_project`.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `expr_node` must be a valid tagged expr pointer.
/// - `alias` must be a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ra_alias_target(
    state: *mut RaParseState,
    expr_node: *mut RaNode,
    alias: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return expr_node;
    };
    if let Some((NodeTag::Expr, idx)) = decode(expr_node) {
        let alias_name = unsafe { c_str_to_string(alias) };
        st.set_expr_alias(idx, alias_name);
    }
    expr_node
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
            st.push_error(format!("ra_join: unknown join type {other}"));
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
            st.push_error(format!("ra_sort: invalid sort key index {idx}"));
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

/// Build an `Intersect` node.
///
/// `all`: 0 = INTERSECT (dedup), nonzero = INTERSECT ALL.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `left` and `right` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_intersect(
    state: *mut RaParseState,
    left: *mut RaNode,
    right: *mut RaNode,
    all: u32,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(left_rel) = decode_rel(st, left) else {
        st.push_error("ra_intersect: invalid left node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(right_rel) = decode_rel(st, right) else {
        st.push_error("ra_intersect: invalid right node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Intersect {
        all: all != 0,
        left: Box::new(left_rel),
        right: Box::new(right_rel),
    })
}

/// Build an `Except` node.
///
/// `all`: 0 = EXCEPT (dedup), nonzero = EXCEPT ALL.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `left` and `right` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_except(
    state: *mut RaParseState,
    left: *mut RaNode,
    right: *mut RaNode,
    all: u32,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(left_rel) = decode_rel(st, left) else {
        st.push_error("ra_except: invalid left node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(right_rel) = decode_rel(st, right) else {
        st.push_error("ra_except: invalid right node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Except {
        all: all != 0,
        left: Box::new(left_rel),
        right: Box::new(right_rel),
    })
}

/// Build a `Values` node.
///
/// `rows_list` is a tagged list of tagged lists. Each inner list contains
/// `Expr` indices representing one row of values.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `rows_list` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_values(
    state: *mut RaParseState,
    rows_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(row_indices) = decode_list(st, rows_list) else {
        st.push_error("ra_values: invalid rows list".to_owned());
        return std::ptr::null_mut();
    };
    let mut rows = Vec::with_capacity(row_indices.len());
    for row_idx in row_indices {
        let Some(col_indices) = st.get_list(row_idx).map(<[usize]>::to_vec) else {
            st.push_error(format!("ra_values: invalid row list index {row_idx}"));
            return std::ptr::null_mut();
        };
        let mut row = Vec::with_capacity(col_indices.len());
        for col_idx in col_indices {
            let Some(expr) = st.take_expr(col_idx) else {
                st.push_error(format!("ra_values: invalid expr index {col_idx}"));
                return std::ptr::null_mut();
            };
            row.push(expr);
        }
        rows.push(row);
    }
    st.push_rel(RelExpr::Values { rows })
}

/// Build a `RecursiveCTE` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must point to at least `name_len` valid bytes.
/// - `base`, `recursive`, `body` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_recursive_cte(
    state: *mut RaParseState,
    name: *const c_char,
    name_len: usize,
    base: *mut RaNode,
    recursive: *mut RaNode,
    body: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let cte_name = unsafe { c_str_len_to_string(name, name_len) };
    let Some(base_rel) = decode_rel(st, base) else {
        st.push_error("ra_recursive_cte: invalid base node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(recursive_rel) = decode_rel(st, recursive) else {
        st.push_error("ra_recursive_cte: invalid recursive node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(body_rel) = decode_rel(st, body) else {
        st.push_error("ra_recursive_cte: invalid body node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::RecursiveCTE {
        name: cte_name,
        base_case: Box::new(base_rel),
        recursive_case: Box::new(recursive_rel),
        body: Box::new(body_rel),
        cycle_detection: Some(CycleDetection {
            track_columns: Vec::new(),
            max_depth: Some(1000),
            cycle_mark_column: None,
            path_column: None,
        }),
    })
}

/// Build a `RecursiveCTE` or `CTE` node from a `WITH RECURSIVE` clause.
///
/// Inspects `cte_body`:
/// - If it is `RelExpr::Union { all: true, .. }`, splits it into base and
///   recursive cases and creates a `RecursiveCTE` node.
/// - Otherwise creates a regular `CTE` node (RECURSIVE keyword was present
///   but the body is not a UNION ALL).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be a valid C string of length `name_len`.
/// - `cte_body` and `query_body` must be valid tagged relational nodes or null.
#[no_mangle]
pub unsafe extern "C" fn ra_recursive_cte_auto(
    state: *mut RaParseState,
    name: *const c_char,
    name_len: usize,
    cte_body: *mut RaNode,
    query_body: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let cte_name = unsafe { c_str_len_to_string(name, name_len) };
    let Some(body_rel) = decode_rel(st, cte_body) else {
        st.push_error("ra_recursive_cte_auto: invalid cte_body node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(query_rel) = decode_rel(st, query_body) else {
        st.push_error("ra_recursive_cte_auto: invalid query_body node".to_owned());
        return std::ptr::null_mut();
    };

    // If the CTE body is a plain UNION (not ALL) inside RECURSIVE, that is
    // invalid SQL: recursive CTEs must use UNION ALL.
    if let RelExpr::Union { all: false, .. } = body_rel {
        st.push_error("WITH RECURSIVE requires UNION ALL, not plain UNION".to_owned());
        return std::ptr::null_mut();
    }

    // If the CTE body is a UNION ALL, split into base and recursive cases.
    if let RelExpr::Union {
        left,
        right,
        all: true,
    } = body_rel
    {
        st.push_rel(RelExpr::RecursiveCTE {
            name: cte_name,
            base_case: left,
            recursive_case: right,
            body: Box::new(query_rel),
            cycle_detection: Some(CycleDetection {
                track_columns: Vec::new(),
                max_depth: Some(1000),
                cycle_mark_column: None,
                path_column: None,
            }),
        })
    } else {
        // Not a UNION ALL: treat as regular CTE (RECURSIVE keyword ignored).
        st.push_rel(RelExpr::CTE {
            name: cte_name,
            definition: Box::new(body_rel),
            body: Box::new(query_rel),
        })
    }
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
pub unsafe extern "C" fn ra_distinct(state: *mut RaParseState, input: *mut RaNode) -> *mut RaNode {
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
// DML builders
// ---------------------------------------------------------------------------

/// Build an `Insert` node.
///
/// - `columns` is a list of column-reference expr indices (or null for all columns).
/// - `source` is a Rel node (VALUES or SELECT).
/// - `on_conflict` is null (no clause), an empty list (DO NOTHING), or a
///   2-element list [`target_cols_list_idx`, `assignments_list_idx`] (DO UPDATE).
/// - `returning` is a list of expr indices (or null for no RETURNING).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - All pointer args must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_insert(
    state: *mut RaParseState,
    table: *const c_char,
    columns: *mut RaNode,
    source: *mut RaNode,
    on_conflict: *mut RaNode,
    returning: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };

    // Decode column list: list of Expr::Column nodes → Vec<String>
    let col_names = if columns.is_null() {
        vec![]
    } else {
        decode_column_names(st, columns)
    };

    // Decode source relation
    let Some(source_rel) = decode_rel(st, source) else {
        st.push_error("ra_insert: invalid source node".to_owned());
        return std::ptr::null_mut();
    };

    // Decode on_conflict
    let on_conflict_val = decode_on_conflict(st, on_conflict);

    // Decode returning
    let returning_val = decode_returning(st, returning);

    st.push_rel(RelExpr::Insert {
        table: table_name,
        columns: col_names,
        source: Box::new(source_rel),
        on_conflict: on_conflict_val,
        returning: returning_val,
    })
}

/// Build an `Update` node.
///
/// - `assignments` is a list of 2-element sub-lists (`col_expr_idx`, `val_expr_idx`).
/// - `filter` is an expr node (or null for no WHERE).
/// - `from` is a Rel node (or null for no FROM).
/// - `returning` is a list of expr indices (or null for no RETURNING).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - All pointer args must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_update(
    state: *mut RaParseState,
    table: *const c_char,
    assignments: *mut RaNode,
    filter: *mut RaNode,
    from: *mut RaNode,
    returning: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };

    // Decode assignments
    let assign_vec = decode_assignments(st, assignments);

    // Decode optional filter
    let filter_val = if filter.is_null() {
        None
    } else {
        decode_expr(st, filter)
    };

    // Decode optional FROM
    let from_val = if from.is_null() {
        None
    } else {
        decode_rel(st, from).map(Box::new)
    };

    // Decode returning
    let returning_val = decode_returning(st, returning);

    st.push_rel(RelExpr::Update {
        table: table_name,
        assignments: assign_vec,
        filter: filter_val,
        from: from_val,
        returning: returning_val,
    })
}

/// Build a `Delete` node.
///
/// - `filter` is an expr node (or null for no WHERE).
/// - `using_clause` is a Rel node (or null for no USING).
/// - `returning` is a list of expr indices (or null for no RETURNING).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - All pointer args must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_delete(
    state: *mut RaParseState,
    table: *const c_char,
    filter: *mut RaNode,
    using_clause: *mut RaNode,
    returning: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let table_name = unsafe { c_str_to_string(table) };

    // Decode optional filter
    let filter_val = if filter.is_null() {
        None
    } else {
        decode_expr(st, filter)
    };

    // Decode optional USING
    let using_val = if using_clause.is_null() {
        None
    } else {
        decode_rel(st, using_clause).map(Box::new)
    };

    // Decode returning
    let returning_val = decode_returning(st, returning);

    st.push_rel(RelExpr::Delete {
        table: table_name,
        filter: filter_val,
        using: using_val,
        returning: returning_val,
    })
}

/// Determine the MERGE match kind from a `BY <ident>` clause: returns
/// 2 (`NotMatchedBySource`) for "source" (case-insensitive), else 1
/// (`NotMatched`, i.e. BY TARGET).
///
/// # Safety
/// - `ident` must be null or a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn ra_merge_kind_by(ident: *const c_char) -> c_int {
    if ident.is_null() {
        return 1;
    }
    let s = unsafe { c_str_to_string(ident) };
    if s.eq_ignore_ascii_case("source") {
        2
    } else {
        1
    }
}

/// Map a kind code to a [`MergeMatchKind`] (0=matched, 1=not matched,
/// 2=not matched by source).
fn merge_kind(code: c_int) -> MergeMatchKind {
    match code {
        1 => MergeMatchKind::NotMatched,
        2 => MergeMatchKind::NotMatchedBySource,
        _ => MergeMatchKind::Matched,
    }
}

/// Decode an optional `AND` condition for a MERGE WHEN clause.
fn merge_cond(state: &RaParseState, cond: *mut RaNode) -> Option<Expr> {
    if cond.is_null() {
        None
    } else {
        decode_expr(state, cond)
    }
}

/// Build a `WHEN [NOT] MATCHED ... THEN UPDATE SET ...` clause.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `cond` / `assignments` must be null or valid tagged pointers.
#[no_mangle]
pub unsafe extern "C" fn ra_merge_when_update(
    state: *mut RaParseState,
    kind: c_int,
    cond: *mut RaNode,
    assignments: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let condition = merge_cond(st, cond);
    st.push_merge_when(MergeWhen {
        kind: merge_kind(kind),
        condition,
        action: MergeAction::Update {
            assignments: decode_assignments(st, assignments),
        },
    })
}

/// Build a `WHEN [NOT] MATCHED ... THEN DELETE` clause.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `cond` must be null or a valid tagged pointer.
#[no_mangle]
pub unsafe extern "C" fn ra_merge_when_delete(
    state: *mut RaParseState,
    kind: c_int,
    cond: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let condition = merge_cond(st, cond);
    st.push_merge_when(MergeWhen {
        kind: merge_kind(kind),
        condition,
        action: MergeAction::Delete,
    })
}

/// Build a `WHEN [NOT] MATCHED ... THEN DO NOTHING` clause.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `cond` must be null or a valid tagged pointer.
#[no_mangle]
pub unsafe extern "C" fn ra_merge_when_nothing(
    state: *mut RaParseState,
    kind: c_int,
    cond: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let condition = merge_cond(st, cond);
    st.push_merge_when(MergeWhen {
        kind: merge_kind(kind),
        condition,
        action: MergeAction::DoNothing,
    })
}

/// Build a `WHEN NOT MATCHED ... THEN INSERT [(cols)] VALUES (...)` clause.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `cond` / `columns` / `values` must be null or valid tagged pointers.
#[no_mangle]
pub unsafe extern "C" fn ra_merge_when_insert(
    state: *mut RaParseState,
    kind: c_int,
    cond: *mut RaNode,
    columns: *mut RaNode,
    values: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let condition = merge_cond(st, cond);
    let cols = if columns.is_null() {
        vec![]
    } else {
        decode_column_names(st, columns)
    };
    let vals = decode_expr_list(st, values);
    st.push_merge_when(MergeWhen {
        kind: merge_kind(kind),
        condition,
        action: MergeAction::Insert {
            columns: cols,
            values: vals,
        },
    })
}

/// Build a `MERGE INTO target USING source ON cond WHEN ...` statement.
///
/// `when_clauses` is a list of MERGE-WHEN tagged node indices.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `source` / `on` / `when_clauses` / `returning` must be valid
///   tagged pointers or null where optional.
#[no_mangle]
pub unsafe extern "C" fn ra_merge(
    state: *mut RaParseState,
    target: *const c_char,
    source: *mut RaNode,
    on: *mut RaNode,
    when_clauses: *mut RaNode,
    returning: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let target_name = unsafe { c_str_to_string(target) };
    let Some(source_rel) = decode_rel(st, source) else {
        st.push_error("ra_merge: invalid source node".to_owned());
        return std::ptr::null_mut();
    };
    let Some(on_expr) = decode_expr(st, on) else {
        st.push_error("ra_merge: invalid ON condition".to_owned());
        return std::ptr::null_mut();
    };
    let whens = decode_merge_whens(st, when_clauses);
    let returning_val = decode_returning(st, returning);

    st.push_rel(RelExpr::Merge {
        target: target_name,
        source: Box::new(source_rel),
        on: on_expr,
        when_clauses: whens,
        returning: returning_val,
    })
}

/// Decode a list of expr indices into `Vec<Expr>`.
fn decode_expr_list(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<Expr> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut out = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(e) = state.take_expr(idx) {
            out.push(e);
        }
    }
    out
}

/// Decode a list of MERGE-WHEN tagged nodes into `Vec<MergeWhen>`.
fn decode_merge_whens(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<MergeWhen> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut out = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(w) = state.take_merge_when(idx) {
            out.push(w);
        }
    }
    out
}

/// Read an optional NUL-terminated C string into `Option<String>`.
///
/// # Safety
/// - `ptr` must be null or a valid NUL-terminated C string.
unsafe fn opt_c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { c_str_to_string(ptr) })
    }
}

/// Build a `GRAPH_TABLE` vertex pattern element `(var IS label)`.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `variable` / `label` must be null or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ra_graph_vertex(
    state: *mut RaParseState,
    variable: *const c_char,
    label: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let variable = unsafe { opt_c_string(variable) };
    let label = unsafe { opt_c_string(label) };
    st.push_graph_elem(GraphPatternElement::Vertex { variable, label })
}

/// Build a `GRAPH_TABLE` edge pattern element. `direction`: 0=right
/// (`-[]->`), 1=left (`<-[]-`), other=undirected (`-[]-`).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `variable` / `label` must be null or valid C strings.
#[no_mangle]
pub unsafe extern "C" fn ra_graph_edge(
    state: *mut RaParseState,
    variable: *const c_char,
    label: *const c_char,
    direction: c_int,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let variable = unsafe { opt_c_string(variable) };
    let label = unsafe { opt_c_string(label) };
    let direction = match direction {
        0 => EdgeDirection::Right,
        1 => EdgeDirection::Left,
        _ => EdgeDirection::Undirected,
    };
    st.push_graph_elem(GraphPatternElement::Edge {
        variable,
        label,
        direction,
    })
}

/// Build a `GRAPH_TABLE (graph MATCH pattern COLUMNS (...))` relation.
///
/// `pattern` is a list of GRAPH-element tagged nodes; `columns` is a
/// target list (as for RETURNING).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `graph` / `alias` must be null or valid C strings.
/// - `pattern` / `columns` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_graph_table(
    state: *mut RaParseState,
    graph: *const c_char,
    pattern: *mut RaNode,
    columns: *mut RaNode,
    alias: *const c_char,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let graph_name = unsafe { c_str_to_string(graph) };
    let pattern_elems = decode_graph_pattern(st, pattern);
    let columns = decode_returning(st, columns).unwrap_or_default();
    let alias = unsafe { opt_c_string(alias) };
    st.push_rel(RelExpr::GraphTable {
        graph: graph_name,
        pattern: pattern_elems,
        columns,
        alias,
    })
}

/// Decode a list of GRAPH-element tagged nodes into a pattern vec.
fn decode_graph_pattern(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<GraphPatternElement> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut out = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(e) = state.take_graph_elem(idx) {
            out.push(e);
        }
    }
    out
}

/// Build an ON CONFLICT DO NOTHING marker (empty list).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_on_conflict_nothing(state: *mut RaParseState) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    // Empty list = DoNothing sentinel
    st.push_list()
}

/// Build an ON CONFLICT DO UPDATE marker.
///
/// Stores a 2-element list: [`target_cols_list_idx`, `assignments_list_idx`].
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `target_cols` and `assignments` must be valid tagged list pointers.
#[no_mangle]
pub unsafe extern "C" fn ra_on_conflict_update(
    state: *mut RaParseState,
    target_cols: *mut RaNode,
    assignments: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some((_tag1, tc_idx)) = decode(target_cols) else {
        st.push_error("ra_on_conflict_update: invalid target_cols".to_owned());
        return std::ptr::null_mut();
    };
    let Some((_tag2, as_idx)) = decode(assignments) else {
        st.push_error("ra_on_conflict_update: invalid assignments".to_owned());
        return std::ptr::null_mut();
    };
    let list_ptr = st.push_list();
    let Some((NodeTag::List, list_idx)) = decode(list_ptr) else {
        return std::ptr::null_mut();
    };
    st.list_push(list_idx, tc_idx);
    st.list_push(list_idx, as_idx);
    list_ptr
}

/// Build an ON CONFLICT DO SELECT marker (`PostgreSQL` 19).
///
/// Stores a single-element list `[target_cols_list_idx]`, distinguishing
/// it from DO NOTHING (0 elements) and DO UPDATE (2 elements).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `target_cols` must be null or a valid tagged list pointer.
#[no_mangle]
pub unsafe extern "C" fn ra_on_conflict_select(
    state: *mut RaParseState,
    target_cols: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let list_ptr = st.push_list();
    let Some((NodeTag::List, list_idx)) = decode(list_ptr) else {
        return std::ptr::null_mut();
    };
    // Bare `ON CONFLICT DO SELECT` (no target) → empty target list.
    let tc_idx = if let Some((_tag, idx)) = decode(target_cols) {
        idx
    } else {
        let empty = st.push_list();
        let Some((NodeTag::List, i)) = decode(empty) else {
            return std::ptr::null_mut();
        };
        i
    };
    st.list_push(list_idx, tc_idx);
    list_ptr
}

/// Build an assignment node (a 2-element list: [`col_name_expr`, `value_expr`]).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `column` must be a valid NUL-terminated C string.
/// - `value` must be a valid tagged expr pointer.
#[no_mangle]
pub unsafe extern "C" fn ra_assignment(
    state: *mut RaParseState,
    column: *const c_char,
    value: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let col_name = unsafe { c_str_to_string(column) };

    // Push column name as a Column expr
    let col_ptr = st.push_expr(Expr::Column(ColumnRef::new(col_name)));
    let Some((_tag_c, col_idx)) = decode(col_ptr) else {
        return std::ptr::null_mut();
    };

    // Get value index
    let Some((_tag_v, val_idx)) = decode(value) else {
        st.push_error("ra_assignment: invalid value node".to_owned());
        return std::ptr::null_mut();
    };

    // Create a 2-element list [col_idx, val_idx]
    let list_ptr = st.push_list();
    let Some((NodeTag::List, list_idx)) = decode(list_ptr) else {
        return std::ptr::null_mut();
    };
    st.list_push(list_idx, col_idx);
    st.list_push(list_idx, val_idx);
    list_ptr
}

/// Build a DEFAULT VALUES source (empty Values node).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_default_values(state: *mut RaParseState) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Values { rows: vec![] })
}

// ---------------------------------------------------------------------------
// DML decode helpers
// ---------------------------------------------------------------------------

/// Decode a list of Column exprs into column name strings.
fn decode_column_names(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<String> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut names = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(Expr::Column(col_ref)) = state.take_expr(idx) {
            names.push(col_ref.column);
        }
    }
    names
}

/// Decode an assignment list (list of 2-element sub-lists) into (`column_name`, Expr) pairs.
fn decode_assignments(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<(String, Expr)> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut result = Vec::with_capacity(indices.len());
    for idx in indices {
        // Each assignment is a 2-element list [col_expr_idx, val_expr_idx]
        let Some(pair) = state.get_list(idx).map(<[usize]>::to_vec) else {
            continue;
        };
        if pair.len() != 2 {
            continue;
        }
        let col_name = if let Some(Expr::Column(col_ref)) = state.take_expr(pair[0]) {
            col_ref.column
        } else {
            continue;
        };
        let Some(val_expr) = state.take_expr(pair[1]) else {
            continue;
        };
        result.push((col_name, val_expr));
    }
    result
}

/// Extract conflict-target column names from a tagged column-list
/// index. Returns an empty vec when the list is absent/empty.
fn decode_target_columns(state: &RaParseState, list_idx: usize) -> Vec<String> {
    if let Some(tc_list) = state.get_list(list_idx).map(<[usize]>::to_vec) {
        let mut names = Vec::with_capacity(tc_list.len());
        for idx in tc_list {
            if let Some(Expr::Column(col_ref)) = state.take_expr(idx) {
                names.push(col_ref.column);
            }
        }
        names
    } else {
        vec![]
    }
}

/// Decode an `on_conflict` pointer into an `Option<OnConflict>`.
///
/// - null → None
/// - empty list → Some(DoNothing)
/// - single-element list → Some(DoSelect { target })  (`PostgreSQL` 19)
/// - 2-element list → Some(DoUpdate { target, assignments })
fn decode_on_conflict(state: &RaParseState, ptr: *mut RaNode) -> Option<OnConflict> {
    if ptr.is_null() {
        return None;
    }
    let items = decode_list(state, ptr)?;
    if items.is_empty() {
        return Some(OnConflict::DoNothing);
    }
    if items.len() == 1 {
        // DO SELECT (PG19): items[0] = target_cols list index.
        let target = decode_target_columns(state, items[0]);
        return Some(OnConflict::DoSelect { target });
    }
    if items.len() == 2 {
        // items[0] = target_cols list index, items[1] = assignments list index
        let target_names = decode_target_columns(state, items[0]);
        let assignments = if let Some(as_list) = state.get_list(items[1]).map(<[usize]>::to_vec) {
            let mut assigns = Vec::with_capacity(as_list.len());
            for idx in as_list {
                if let Some(pair) = state.get_list(idx).map(<[usize]>::to_vec) {
                    if pair.len() == 2 {
                        let col_name = if let Some(Expr::Column(col_ref)) = state.take_expr(pair[0])
                        {
                            col_ref.column
                        } else {
                            continue;
                        };
                        let Some(val_expr) = state.take_expr(pair[1]) else {
                            continue;
                        };
                        assigns.push((col_name, val_expr));
                    }
                }
            }
            assigns
        } else {
            vec![]
        };
        return Some(OnConflict::DoUpdate {
            target: target_names,
            assignments,
        });
    }
    None
}

/// Decode a RETURNING target list into `Option<Vec<ProjectionColumn>>`.
fn decode_returning(state: &RaParseState, ptr: *mut RaNode) -> Option<Vec<ProjectionColumn>> {
    if ptr.is_null() {
        return None;
    }
    let exprs = collect_exprs(state, ptr);
    if exprs.is_empty() {
        return None;
    }
    Some(
        exprs
            .into_iter()
            .map(|expr| ProjectionColumn { expr, alias: None })
            .collect(),
    )
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
pub unsafe extern "C" fn ra_column(state: *mut RaParseState, name: *const c_char) -> *mut RaNode {
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
pub unsafe extern "C" fn ra_const_int(state: *mut RaParseState, value: i64) -> *mut RaNode {
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
pub unsafe extern "C" fn ra_const_float(state: *mut RaParseState, value: f64) -> *mut RaNode {
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
pub unsafe extern "C" fn ra_const_null(state: *mut RaParseState) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::Const(Const::Null))
}

/// Build a boolean constant expression.
///
/// `value`: 0 = false, nonzero = true.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_const_bool(state: *mut RaParseState, value: u32) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::Const(Const::Bool(value != 0)))
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
            st.push_error(format!("ra_binop: unknown operator {other}"));
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

/// Build a unary operation expression.
///
/// `op_code` encoding: 0=Not, 1=IsNull, 2=IsNotNull, 3=Neg.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `operand` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_unary_op(
    state: *mut RaParseState,
    op_code: u32,
    operand: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let op = match op_code {
        0 => UnaryOp::Not,
        1 => UnaryOp::IsNull,
        2 => UnaryOp::IsNotNull,
        3 => UnaryOp::Neg,
        other => {
            st.push_error(format!("ra_unary_op: unknown operator {other}"));
            return std::ptr::null_mut();
        }
    };
    let Some(operand_expr) = decode_expr(st, operand) else {
        st.push_error("ra_unary_op: invalid operand node".to_owned());
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::UnaryOp {
        op,
        operand: Box::new(operand_expr),
    })
}

/// Build a CASE expression.
///
/// `operand` is nullable (null for searched CASE, non-null for simple CASE).
/// `when_list` is a tagged list of alternating condition/result `Expr` nodes
/// (must contain an even number of elements).
/// `else_expr` is nullable (null for no ELSE clause).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `operand`, `when_list`, `else_expr` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_case(
    state: *mut RaParseState,
    operand: *mut RaNode,
    when_list: *mut RaNode,
    else_expr: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let case_operand = if operand.is_null() {
        None
    } else {
        let Some(expr) = decode_expr(st, operand) else {
            st.push_error("ra_case: invalid operand node".to_owned());
            return std::ptr::null_mut();
        };
        Some(Box::new(expr))
    };
    let Some(indices) = decode_list(st, when_list) else {
        st.push_error("ra_case: invalid when list".to_owned());
        return std::ptr::null_mut();
    };
    if indices.len() % 2 != 0 {
        st.push_error("ra_case: when list must have even length".to_owned());
        return std::ptr::null_mut();
    }
    let mut when_clauses = Vec::with_capacity(indices.len() / 2);
    let mut iter = indices.into_iter();
    while let Some(cond_idx) = iter.next() {
        let Some(result_idx) = iter.next() else {
            st.push_error("ra_case: when list truncated".to_owned());
            return std::ptr::null_mut();
        };
        let Some(cond) = st.take_expr(cond_idx) else {
            st.push_error(format!("ra_case: invalid condition index {cond_idx}"));
            return std::ptr::null_mut();
        };
        let Some(result) = st.take_expr(result_idx) else {
            st.push_error(format!("ra_case: invalid result index {result_idx}"));
            return std::ptr::null_mut();
        };
        when_clauses.push((cond, result));
    }
    let else_result = if else_expr.is_null() {
        None
    } else {
        let Some(expr) = decode_expr(st, else_expr) else {
            st.push_error("ra_case: invalid else expression".to_owned());
            return std::ptr::null_mut();
        };
        Some(Box::new(expr))
    };
    st.push_expr(Expr::Case {
        operand: case_operand,
        when_clauses,
        else_result,
    })
}

/// Build a CAST expression.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `expr` must be a valid tagged pointer or null.
/// - `type_str` must point to at least `type_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn ra_cast(
    state: *mut RaParseState,
    expr: *mut RaNode,
    type_str: *const c_char,
    type_len: usize,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(inner_expr) = decode_expr(st, expr) else {
        st.push_error("ra_cast: invalid expression node".to_owned());
        return std::ptr::null_mut();
    };
    let target_type = unsafe { c_str_len_to_string(type_str, type_len) };
    st.push_expr(Expr::Cast {
        expr: Box::new(inner_expr),
        target_type,
    })
}

/// Build a `SubQuery` expression.
///
/// `type_code` encoding: 0=Scalar, 1=Exists, 2=In, 3=Any, 4=All.
/// `rel_node` is the subquery `RelExpr`.
/// `test_expr` is nullable (the left-hand expression for IN/ANY/ALL).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `rel_node` must be a valid tagged Rel pointer or null.
/// - `test_expr` must be a valid tagged Expr pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_subquery(
    state: *mut RaParseState,
    type_code: u32,
    rel_node: *mut RaNode,
    test_expr: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let subquery_type = match type_code {
        0 => SubQueryType::Scalar,
        1 => SubQueryType::Exists,
        2 => SubQueryType::In,
        3 => SubQueryType::Any,
        4 => SubQueryType::All,
        other => {
            st.push_error(format!("ra_subquery: unknown type code {other}"));
            return std::ptr::null_mut();
        }
    };
    let Some(query_rel) = decode_rel(st, rel_node) else {
        st.push_error("ra_subquery: invalid rel node".to_owned());
        return std::ptr::null_mut();
    };
    let test = if test_expr.is_null() {
        None
    } else {
        let Some(expr) = decode_expr(st, test_expr) else {
            st.push_error("ra_subquery: invalid test expression".to_owned());
            return std::ptr::null_mut();
        };
        Some(Box::new(expr))
    };
    st.push_expr(Expr::SubQuery {
        subquery_type,
        query: Box::new(query_rel),
        test_expr: test,
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

/// Build an Array constructor expression.
///
/// `elem_list` is a tagged list of `Expr` indices.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `elem_list` must be a valid tagged pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_array(state: *mut RaParseState, elem_list: *mut RaNode) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let elements = collect_exprs(st, elem_list);
    st.push_expr(Expr::Array(elements))
}

/// Build an `ArrayIndex` expression (e.g., `arr[2]`).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `array_expr` and `index_expr` must be valid tagged pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_array_index(
    state: *mut RaParseState,
    array_expr: *mut RaNode,
    index_expr: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(array) = decode_expr(st, array_expr) else {
        st.push_error("ra_array_index: invalid array expression".to_owned());
        return std::ptr::null_mut();
    };
    let Some(index) = decode_expr(st, index_expr) else {
        st.push_error("ra_array_index: invalid index expression".to_owned());
        return std::ptr::null_mut();
    };
    st.push_expr(Expr::ArrayIndex(Box::new(array), Box::new(index)))
}

/// Build an `Unnest` relational node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `array_expr` must be a valid tagged expression pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_unnest(
    state: *mut RaParseState,
    array_expr: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(expr) = decode_expr(st, array_expr) else {
        st.push_error("ra_unnest: invalid array expression".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::unnest(expr, None))
}

/// Build an `Unnest` relational node with `WITH ORDINALITY`.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `array_expr` must be a valid tagged expression pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_unnest_ord(
    state: *mut RaParseState,
    array_expr: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(expr) = decode_expr(st, array_expr) else {
        st.push_error("ra_unnest_ord: invalid array expression".to_owned());
        return std::ptr::null_mut();
    };
    st.push_rel(RelExpr::Unnest {
        expr,
        alias: None,
        input: None,
        with_ordinality: true,
    })
}

/// Build a `TableFunction` relational node (for `generate_series` etc.).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be a valid C string of length `name_len`.
/// - `args_list` must be a valid tagged list pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_table_function(
    state: *mut RaParseState,
    name: *const c_char,
    name_len: usize,
    args_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let func_name = unsafe { c_str_len_to_string(name, name_len) };
    let args = collect_exprs(st, args_list);
    st.push_rel(RelExpr::table_function(func_name, args, vec![]))
}

/// Build a window function marker expression.
///
/// The function name is prefixed with `__window_` so the post-parse
/// transformer can detect it and promote the result to a `Window` node.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be a valid NUL-terminated C string.
/// - `args_list` must be a valid tagged list pointer or null.
#[no_mangle]
pub unsafe extern "C" fn ra_window_marker(
    state: *mut RaParseState,
    name: *const c_char,
    args_list: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let raw_name = unsafe { c_str_to_string(name) };
    let marker_name = format!("__window_{raw_name}");
    let args = collect_exprs(st, args_list);
    let func_name = marker_name;
    st.push_expr(Expr::Function {
        name: func_name,
        args,
    })
}

/// Build a window function marker expression with partition and order info.
///
/// Encodes partition and order lists as sentinel args appended after the
/// real function args so the post-parse transformer can reconstruct the
/// full OVER clause:
/// - `__window_partition(exprs...)` — one sentinel per partition expr
/// - `__window_order_asc(expr)` or `__window_order_desc(expr)` — one per
///   sort key, encoding direction.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `name` must be a valid NUL-terminated C string.
/// - `args_list`, `partition_list`, `order_list` must be valid tagged list
///   pointers or null.
#[no_mangle]
pub unsafe extern "C" fn ra_window_marker_full(
    state: *mut RaParseState,
    name: *const c_char,
    args_list: *mut RaNode,
    partition_list: *mut RaNode,
    order_list: *mut RaNode,
    has_frame: c_int,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let raw_name = unsafe { c_str_to_string(name) };
    let marker_name = format!("__window_{raw_name}");
    let mut args = collect_exprs(st, args_list);

    // Encode partition exprs as a single __window_partition sentinel arg.
    let partition_exprs = collect_exprs(st, partition_list);
    if !partition_exprs.is_empty() {
        args.push(Expr::Function {
            name: "__window_partition".to_string(),
            args: partition_exprs,
        });
    }

    // Encode each sort key as __window_order_asc(expr) or __window_order_desc(expr).
    let order_keys = collect_sort_keys(st, order_list);
    for key in order_keys {
        let sentinel_name = match key.direction {
            SortDirection::Asc => "__window_order_asc",
            SortDirection::Desc => "__window_order_desc",
        };
        args.push(Expr::Function {
            name: sentinel_name.to_string(),
            args: vec![key.expr],
        });
    }

    // An explicit window frame is encoded as a sentinel arg so the
    // optimizer/plan-builder can detect it and defer (the frame semantics
    // are otherwise dropped). Frame-insensitive ranking functions ignore it.
    if has_frame != 0 {
        args.push(Expr::Function {
            name: "__window_frame".to_string(),
            args: Vec::new(),
        });
    }

    st.push_expr(Expr::Function {
        name: marker_name,
        args,
    })
}

/// Build a `FieldAccess` expression (e.g., `(row).field_name`).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `expr` must be a valid tagged pointer or null.
/// - `field_name` must point to at least `field_len` valid bytes.
#[no_mangle]
pub unsafe extern "C" fn ra_field_access(
    state: *mut RaParseState,
    expr: *mut RaNode,
    field_name: *const c_char,
    field_len: usize,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some(base_expr) = decode_expr(st, expr) else {
        st.push_error("ra_field_access: invalid expression node".to_owned());
        return std::ptr::null_mut();
    };
    let name = unsafe { c_str_len_to_string(field_name, field_len) };
    st.push_expr(Expr::FieldAccess {
        expr: Box::new(base_expr),
        field_name: name,
    })
}

// ---------------------------------------------------------------------------
// Aggregate and window expression builders
// ---------------------------------------------------------------------------

/// Build an `AggregateExpr` node.
///
/// `func_code` encoding: 0=Count, 1=Sum, 2=Avg, 3=Min, 4=Max, 5=StdDev,
/// 6=Variance, 7=StringAgg, 8=ArrayAgg.
/// `arg` is nullable (null for COUNT(*) with no argument).
/// `distinct`: 0 = no DISTINCT, nonzero = DISTINCT.
/// `alias`/`alias_len`: optional output alias (null/0 for none).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `arg` must be a valid tagged Expr pointer or null.
/// - `alias` must point to at least `alias_len` valid bytes, or be null.
#[no_mangle]
pub unsafe extern "C" fn ra_agg_expr(
    state: *mut RaParseState,
    func_code: u32,
    arg: *mut RaNode,
    distinct: u32,
    alias: *const c_char,
    alias_len: usize,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let function = match func_code {
        0 => AggregateFunction::Count,
        1 => AggregateFunction::Sum,
        2 => AggregateFunction::Avg,
        3 => AggregateFunction::Min,
        4 => AggregateFunction::Max,
        5 => AggregateFunction::StdDev,
        6 => AggregateFunction::Variance,
        7 => AggregateFunction::StringAgg,
        8 => AggregateFunction::ArrayAgg,
        other => {
            st.push_error(format!("ra_agg_expr: unknown function code {other}"));
            return std::ptr::null_mut();
        }
    };
    let arg_expr = if arg.is_null() {
        None
    } else {
        let Some(expr) = decode_expr(st, arg) else {
            st.push_error("ra_agg_expr: invalid arg expression".to_owned());
            return std::ptr::null_mut();
        };
        Some(expr)
    };
    let alias_str = unsafe { c_str_len_to_string(alias, alias_len) };
    let alias_opt = if alias_str.is_empty() {
        None
    } else {
        Some(alias_str)
    };
    st.push_agg_expr(AggregateExpr {
        function,
        arg: arg_expr,
        distinct: distinct != 0,
        alias: alias_opt,
    })
}

/// Build a `WindowExpr` node.
///
/// `func_code` encoding: 0=Avg, 1=Sum, 2=Count, 3=Min, 4=Max,
/// 5=RowNumber, 6=Rank, 7=DenseRank, 8=PercentRank, 9=Ntile,
/// 10=Lag, 11=Lead, 12=FirstValue, 13=LastValue, 14=NthValue.
/// `arg` is nullable.
/// `partition_list` is a tagged list of `Expr` indices for PARTITION BY.
/// `order_list` is a tagged list of `SortKey` indices for ORDER BY.
/// `alias`/`alias_len`: optional output alias.
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `arg`, `partition_list`, `order_list` must be valid tagged ptrs or null.
/// - `alias` must point to at least `alias_len` valid bytes, or be null.
#[no_mangle]
pub unsafe extern "C" fn ra_window_expr(
    state: *mut RaParseState,
    func_code: u32,
    arg: *mut RaNode,
    partition_list: *mut RaNode,
    order_list: *mut RaNode,
    alias: *const c_char,
    alias_len: usize,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let function = match func_code {
        0 => WindowFunction::Avg,
        1 => WindowFunction::Sum,
        2 => WindowFunction::Count,
        3 => WindowFunction::Min,
        4 => WindowFunction::Max,
        5 => WindowFunction::RowNumber,
        6 => WindowFunction::Rank,
        7 => WindowFunction::DenseRank,
        8 => WindowFunction::PercentRank,
        9 => WindowFunction::Ntile,
        10 => WindowFunction::Lag,
        11 => WindowFunction::Lead,
        12 => WindowFunction::FirstValue,
        13 => WindowFunction::LastValue,
        14 => WindowFunction::NthValue,
        other => {
            st.push_error(format!("ra_window_expr: unknown function code {other}"));
            return std::ptr::null_mut();
        }
    };
    let arg_expr = if arg.is_null() {
        None
    } else {
        let Some(expr) = decode_expr(st, arg) else {
            st.push_error("ra_window_expr: invalid arg expression".to_owned());
            return std::ptr::null_mut();
        };
        Some(expr)
    };
    let partition_by = collect_exprs(st, partition_list);
    let order_by = collect_sort_keys(st, order_list);
    let alias_str = unsafe { c_str_len_to_string(alias, alias_len) };
    let alias_opt = if alias_str.is_empty() {
        None
    } else {
        Some(alias_str)
    };
    st.push_window_expr(WindowExpr {
        function,
        arg: arg_expr,
        partition_by,
        order_by,
        frame: None,
        alias: alias_opt,
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
pub unsafe extern "C" fn ra_list_new(state: *mut RaParseState) -> *mut RaNode {
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
        st.push_error(format!("ra_list_push: list index {list_idx} out of bounds"));
        std::ptr::null_mut()
    }
}

/// Prepend an item to the front of a list node (returns the same list).
///
/// # Safety
/// - `state` must be null or a valid `*mut RaParseState`.
/// - `list` must be a valid List tagged pointer; `item` a valid tagged pointer.
#[no_mangle]
pub unsafe extern "C" fn ra_list_prepend(
    state: *mut RaParseState,
    list: *mut RaNode,
    item: *mut RaNode,
) -> *mut RaNode {
    let Some(st) = (unsafe { state_ref(state) }) else {
        return std::ptr::null_mut();
    };
    let Some((NodeTag::List, list_idx)) = decode(list) else {
        st.push_error("ra_list_prepend: invalid list pointer".to_owned());
        return std::ptr::null_mut();
    };
    let Some((_tag, item_idx)) = decode(item) else {
        st.push_error("ra_list_prepend: invalid item pointer".to_owned());
        return std::ptr::null_mut();
    };
    if st.list_prepend(list_idx, item_idx) {
        list
    } else {
        st.push_error(format!("ra_list_prepend: list index {list_idx} out of bounds"));
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
        st.push_error("ra_sort_key: invalid expression node".to_owned());
        return std::ptr::null_mut();
    };
    let direction = if ascending != 0 {
        SortDirection::Asc
    } else {
        SortDirection::Desc
    };
    let nulls = match nulls_first {
        0 => NullOrdering::Last,
        1 => NullOrdering::First,
        // 2 = unspecified: PG defaults ASC→NULLS LAST, DESC→NULLS FIRST.
        _ => match direction {
            SortDirection::Asc => NullOrdering::Last,
            SortDirection::Desc => NullOrdering::First,
        },
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
fn collect_exprs(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<Expr> {
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

/// Collect `SortKey` values from a tagged list pointer.
///
/// Returns an empty vec if the pointer is null or not a valid list.
fn collect_sort_keys(state: &RaParseState, list_ptr: *mut RaNode) -> Vec<SortKey> {
    let Some(indices) = decode_list(state, list_ptr) else {
        return vec![];
    };
    let mut result = Vec::with_capacity(indices.len());
    for idx in indices {
        if let Some(key) = state.take_sort_key(idx) {
            result.push(key);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Error recording (called from %syntax_error / %parse_failure)
// ---------------------------------------------------------------------------

/// Record a structured syntax error from the Lime `%syntax_error` hook.
///
/// This is called from the generated C parser when a token cannot be
/// shifted or reduced. It uses the diagnostics API to query the parser
/// state for expected tokens and builds a `StructuredParseError`.
///
/// # Safety
/// - `pstate` must be null or a valid `*mut RaParseState`.
/// - `parser` must be a valid Lime parser handle from `raAlloc`.
/// - `token` must be a valid `RaToken` (passed by value from C).
#[no_mangle]
pub unsafe extern "C" fn ra_record_parse_error(
    pstate: *mut RaParseState,
    token_code: c_int,
    token: RaToken,
    parser: *mut c_void,
) {
    let Some(st) = (unsafe { state_ref(pstate) }) else {
        return;
    };

    let rejected_name = if token_code == 0 {
        "end of input".to_owned()
    } else {
        diagnostics::token_name(token_code)
            .unwrap_or("unknown")
            .to_owned()
    };

    // SAFETY: parser is a valid Lime parser handle from the
    // %syntax_error block — it's the `yypParser` pointer.
    let expected = if let Some(stateno) = unsafe { diagnostics::parser_state(parser) } {
        diagnostics::expected_tokens(stateno)
            .into_iter()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };

    let token_text = if token.text.is_null() {
        None
    } else {
        // SAFETY: token.text is a valid NUL-terminated C string.
        Some(
            unsafe { CStr::from_ptr(token.text) }
                .to_string_lossy()
                .into_owned(),
        )
    };

    let position = usize::try_from(token.location).unwrap_or(0);
    let token_length = usize::try_from(token.length).unwrap_or(1);

    let message = if let Some(ref text) = token_text {
        format!("syntax error: unexpected {rejected_name} '{text}'")
    } else {
        format!("syntax error: unexpected {rejected_name}")
    };

    st.push_structured_error(StructuredParseError {
        position,
        token_length,
        token_text,
        token_name: rejected_name,
        message,
        expected_tokens: expected,
    });
}

/// Record a parse failure (unrecoverable) from the `%parse_failure` hook.
///
/// # Safety
/// - `pstate` must be null or a valid `*mut RaParseState`.
#[no_mangle]
pub unsafe extern "C" fn ra_record_parse_failure(pstate: *mut RaParseState) {
    let Some(st) = (unsafe { state_ref(pstate) }) else {
        return;
    };

    st.push_structured_error(StructuredParseError {
        position: 0,
        token_length: 0,
        token_text: None,
        token_name: String::new(),
        message: "parse failed: unable to recover from syntax error".to_owned(),
        expected_tokens: Vec::new(),
    });
}

#[cfg(test)]
#[expect(
    clippy::panic,
    clippy::expect_used,
    clippy::approx_constant,
    reason = "test code uses panic for assertions and expect for unwrapping"
)]
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

    // -----------------------------------------------------------------------
    // Existing builder tests
    // -----------------------------------------------------------------------

    #[test]
    fn null_state_returns_null() {
        let result = unsafe { ra_scan(std::ptr::null_mut(), std::ptr::null()) };
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
        assert!(matches!(rel, RelExpr::Scan { table, .. } if table == "users"));
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
        let join = unsafe { ra_join(st, 0, left, right, cond) };
        assert!(!join.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(
            rel,
            RelExpr::Join {
                join_type: JoinType::Inner,
                ..
            }
        ));
    }

    #[test]
    fn limit_builds_correctly() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let limited = unsafe { ra_limit(st, scan, 10, 5) };
        assert!(!limited.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(
            rel,
            RelExpr::Limit {
                count: 10,
                offset: 5,
                ..
            }
        ));
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
        assert!(matches!(rel, RelExpr::Union { all: true, .. }));
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
        assert!(matches!(expr, Expr::Const(Const::Float(v)) if (v - 3.14).abs() < f64::EPSILON));
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
        let node = unsafe { ra_qualified_column(st, c"users".as_ptr(), c"id".as_ptr()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Column(ColumnRef::qualified("users", "id")));
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
        assert!(matches!(rel, RelExpr::CTE { name, .. } if name == "temp"));
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
        assert!(
            errs.as_strings().expect("should be string errors")[0].contains("unknown join type")
        );
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
        assert!(errs.as_strings().expect("should be string errors")[0].contains("unknown operator"));
    }

    // -----------------------------------------------------------------------
    // New builder tests: Priority 1 Expr builders
    // -----------------------------------------------------------------------

    #[test]
    fn const_bool_true() {
        let st = new_state();
        let node = unsafe { ra_const_bool(st, 1) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Const(Const::Bool(true)));
    }

    #[test]
    fn const_bool_false() {
        let st = new_state();
        let node = unsafe { ra_const_bool(st, 0) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert_eq!(expr, Expr::Const(Const::Bool(false)));
    }

    #[test]
    fn const_bool_null_state() {
        let result = unsafe { ra_const_bool(std::ptr::null_mut(), 1) };
        assert!(result.is_null());
    }

    #[test]
    fn unary_op_not() {
        let st = new_state();
        let inner = unsafe { ra_const_bool(st, 1) };
        let node = unsafe { ra_unary_op(st, 0, inner) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Not,
                ..
            }
        ));
    }

    #[test]
    fn unary_op_is_null() {
        let st = new_state();
        let col = unsafe { ra_column(st, c"x".as_ptr()) };
        let node = unsafe { ra_unary_op(st, 1, col) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::IsNull,
                ..
            }
        ));
    }

    #[test]
    fn unary_op_is_not_null() {
        let st = new_state();
        let col = unsafe { ra_column(st, c"x".as_ptr()) };
        let node = unsafe { ra_unary_op(st, 2, col) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::IsNotNull,
                ..
            }
        ));
    }

    #[test]
    fn unary_op_neg() {
        let st = new_state();
        let val = unsafe { ra_const_int(st, 42) };
        let node = unsafe { ra_unary_op(st, 3, val) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                ..
            }
        ));
    }

    #[test]
    fn unary_op_invalid_code() {
        let st = new_state();
        let val = unsafe { ra_const_int(st, 1) };
        let node = unsafe { ra_unary_op(st, 99, val) };
        assert!(node.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs.as_strings().expect("should be string errors")[0].contains("unknown operator"));
    }

    #[test]
    fn unary_op_null_state() {
        let result = unsafe { ra_unary_op(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
        assert!(result.is_null());
    }

    #[test]
    fn case_searched() {
        let st = new_state();
        // CASE WHEN x > 0 THEN 'positive' ELSE 'non-positive' END
        let col = unsafe { ra_column(st, c"x".as_ptr()) };
        let zero = unsafe { ra_const_int(st, 0) };
        let cond = unsafe { ra_binop(st, 8, col, zero) }; // Gt
        let result = unsafe { ra_const_str(st, c"positive".as_ptr()) };
        let else_val = unsafe { ra_const_str(st, c"non-positive".as_ptr()) };

        let when_list = unsafe { ra_list_new(st) };
        let when_list = unsafe { ra_list_push(st, when_list, cond) };
        let when_list = unsafe { ra_list_push(st, when_list, result) };

        let node = unsafe { ra_case(st, std::ptr::null_mut(), when_list, else_val) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::Case {
            operand,
            when_clauses,
            else_result,
        } = &expr
        {
            assert!(operand.is_none());
            assert_eq!(when_clauses.len(), 1);
            assert!(else_result.is_some());
        } else {
            panic!("expected Case variant");
        }
    }

    #[test]
    fn case_simple_with_operand() {
        let st = new_state();
        // CASE x WHEN 1 THEN 'one' END
        let operand = unsafe { ra_column(st, c"x".as_ptr()) };
        let when_val = unsafe { ra_const_int(st, 1) };
        let result = unsafe { ra_const_str(st, c"one".as_ptr()) };

        let when_list = unsafe { ra_list_new(st) };
        let when_list = unsafe { ra_list_push(st, when_list, when_val) };
        let when_list = unsafe { ra_list_push(st, when_list, result) };

        let node = unsafe { ra_case(st, operand, when_list, std::ptr::null_mut()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::Case {
            operand,
            when_clauses,
            else_result,
        } = &expr
        {
            assert!(operand.is_some());
            assert_eq!(when_clauses.len(), 1);
            assert!(else_result.is_none());
        } else {
            panic!("expected Case variant");
        }
    }

    #[test]
    fn case_odd_when_list_records_error() {
        let st = new_state();
        let val = unsafe { ra_const_int(st, 1) };
        let when_list = unsafe { ra_list_new(st) };
        let when_list = unsafe { ra_list_push(st, when_list, val) };
        let node = unsafe { ra_case(st, std::ptr::null_mut(), when_list, std::ptr::null_mut()) };
        assert!(node.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs.as_strings().expect("should be string errors")[0].contains("even length"));
    }

    #[test]
    fn case_null_state() {
        let result = unsafe {
            ra_case(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn cast_builds_correctly() {
        let st = new_state();
        let val = unsafe { ra_const_int(st, 42) };
        let type_name = b"VARCHAR";
        let node = unsafe {
            ra_cast(
                st,
                val,
                type_name.as_ptr().cast::<c_char>(),
                type_name.len(),
            )
        };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::Cast {
            target_type,
            expr: inner,
            ..
        } = &expr
        {
            assert_eq!(target_type, "VARCHAR");
            assert_eq!(*inner, Box::new(Expr::Const(Const::Int(42))));
        } else {
            panic!("expected Cast variant");
        }
    }

    #[test]
    fn cast_null_state() {
        let result = unsafe {
            ra_cast(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn subquery_scalar() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let node = unsafe { ra_subquery(st, 0, scan, std::ptr::null_mut()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::SubQuery {
            subquery_type,
            test_expr,
            ..
        } = &expr
        {
            assert_eq!(*subquery_type, SubQueryType::Scalar);
            assert!(test_expr.is_none());
        } else {
            panic!("expected SubQuery variant");
        }
    }

    #[test]
    fn subquery_exists() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let node = unsafe { ra_subquery(st, 1, scan, std::ptr::null_mut()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(
            expr,
            Expr::SubQuery {
                subquery_type: SubQueryType::Exists,
                ..
            }
        ));
    }

    #[test]
    fn subquery_in_with_test_expr() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let test = unsafe { ra_column(st, c"x".as_ptr()) };
        let node = unsafe { ra_subquery(st, 2, scan, test) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::SubQuery {
            subquery_type,
            test_expr,
            ..
        } = &expr
        {
            assert_eq!(*subquery_type, SubQueryType::In);
            assert!(test_expr.is_some());
        } else {
            panic!("expected SubQuery variant");
        }
    }

    #[test]
    fn subquery_invalid_type_records_error() {
        let st = new_state();
        let scan = unsafe { ra_scan(st, c"t".as_ptr()) };
        let node = unsafe { ra_subquery(st, 99, scan, std::ptr::null_mut()) };
        assert!(node.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(
            errs.as_strings().expect("should be string errors")[0].contains("unknown type code")
        );
    }

    #[test]
    fn subquery_null_state() {
        let result = unsafe {
            ra_subquery(
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // New builder tests: Priority 1 RelExpr builders
    // -----------------------------------------------------------------------

    #[test]
    fn intersect_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let node = unsafe { ra_intersect(st, left, right, 0) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Intersect { all: false, .. }));
    }

    #[test]
    fn intersect_all_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let node = unsafe { ra_intersect(st, left, right, 1) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Intersect { all: true, .. }));
    }

    #[test]
    fn intersect_null_state() {
        let result = unsafe {
            ra_intersect(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn except_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let node = unsafe { ra_except(st, left, right, 0) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Except { all: false, .. }));
    }

    #[test]
    fn except_all_builds_correctly() {
        let st = new_state();
        let left = unsafe { ra_scan(st, c"a".as_ptr()) };
        let right = unsafe { ra_scan(st, c"b".as_ptr()) };
        let node = unsafe { ra_except(st, left, right, 1) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        assert!(matches!(rel, RelExpr::Except { all: true, .. }));
    }

    #[test]
    fn except_null_state() {
        let result = unsafe {
            ra_except(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn values_builds_correctly() {
        let st = new_state();
        // VALUES (1, 'a'), (2, 'b')
        let v1 = unsafe { ra_const_int(st, 1) };
        let v2 = unsafe { ra_const_str(st, c"a".as_ptr()) };
        let row1 = unsafe { ra_list_new(st) };
        let row1 = unsafe { ra_list_push(st, row1, v1) };
        let row1 = unsafe { ra_list_push(st, row1, v2) };

        let v3 = unsafe { ra_const_int(st, 2) };
        let v4 = unsafe { ra_const_str(st, c"b".as_ptr()) };
        let row2 = unsafe { ra_list_new(st) };
        let row2 = unsafe { ra_list_push(st, row2, v3) };
        let row2 = unsafe { ra_list_push(st, row2, v4) };

        let rows = unsafe { ra_list_new(st) };
        let rows = unsafe { ra_list_push(st, rows, row1) };
        let rows = unsafe { ra_list_push(st, rows, row2) };

        let node = unsafe { ra_values(st, rows) };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        if let RelExpr::Values { rows } = &rel {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 2);
            assert_eq!(rows[1].len(), 2);
        } else {
            panic!("expected Values variant");
        }
    }

    #[test]
    fn values_null_state() {
        let result = unsafe { ra_values(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(result.is_null());
    }

    #[test]
    fn recursive_cte_builds_correctly() {
        let st = new_state();
        let base = unsafe { ra_scan(st, c"edges".as_ptr()) };
        let recursive = unsafe { ra_scan(st, c"edges".as_ptr()) };
        let body = unsafe { ra_scan(st, c"reachable".as_ptr()) };
        let name = b"reachable";
        let node = unsafe {
            ra_recursive_cte(
                st,
                name.as_ptr().cast::<c_char>(),
                name.len(),
                base,
                recursive,
                body,
            )
        };
        assert!(!node.is_null());

        let state = unsafe { free_state(st) };
        let rel = state.take_result().expect("should succeed");
        if let RelExpr::RecursiveCTE {
            name,
            cycle_detection,
            ..
        } = &rel
        {
            assert_eq!(name, "reachable");
            // Builder now sets a default cycle detection with max_depth=1000.
            let cd = cycle_detection
                .as_ref()
                .expect("cycle_detection should be Some");
            assert_eq!(cd.max_depth, Some(1000));
        } else {
            panic!("expected RecursiveCTE variant");
        }
    }

    #[test]
    fn recursive_cte_null_state() {
        let result = unsafe {
            ra_recursive_cte(
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // New builder tests: AggregateExpr and WindowExpr
    // -----------------------------------------------------------------------

    #[test]
    fn agg_expr_count_star() {
        let st = new_state();
        let node = unsafe { ra_agg_expr(st, 0, std::ptr::null_mut(), 0, std::ptr::null(), 0) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Agg);
        let state = unsafe { free_state(st) };
        let agg = state.take_agg_expr(idx).expect("should exist");
        assert_eq!(agg.function, AggregateFunction::Count);
        assert!(agg.arg.is_none());
        assert!(!agg.distinct);
        assert!(agg.alias.is_none());
    }

    #[test]
    fn agg_expr_sum_distinct_with_alias() {
        let st = new_state();
        let col = unsafe { ra_column(st, c"amount".as_ptr()) };
        let alias = b"total";
        let node =
            unsafe { ra_agg_expr(st, 1, col, 1, alias.as_ptr().cast::<c_char>(), alias.len()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Agg);
        let state = unsafe { free_state(st) };
        let agg = state.take_agg_expr(idx).expect("should exist");
        assert_eq!(agg.function, AggregateFunction::Sum);
        assert!(agg.arg.is_some());
        assert!(agg.distinct);
        assert_eq!(agg.alias.as_deref(), Some("total"));
    }

    #[test]
    fn agg_expr_all_functions() {
        let st = new_state();
        let expected = [
            (0_u32, AggregateFunction::Count),
            (1, AggregateFunction::Sum),
            (2, AggregateFunction::Avg),
            (3, AggregateFunction::Min),
            (4, AggregateFunction::Max),
            (5, AggregateFunction::StdDev),
            (6, AggregateFunction::Variance),
            (7, AggregateFunction::StringAgg),
            (8, AggregateFunction::ArrayAgg),
        ];
        for (code, func) in expected {
            let node =
                unsafe { ra_agg_expr(st, code, std::ptr::null_mut(), 0, std::ptr::null(), 0) };
            assert!(!node.is_null());
            let (tag, idx) = decode(node).expect("should decode");
            assert_eq!(tag, NodeTag::Agg);
            let agg = unsafe { &*st }.take_agg_expr(idx).expect("should exist");
            assert_eq!(agg.function, func);
        }
        unsafe { free_state(st) };
    }

    #[test]
    fn agg_expr_invalid_function_records_error() {
        let st = new_state();
        let node = unsafe { ra_agg_expr(st, 99, std::ptr::null_mut(), 0, std::ptr::null(), 0) };
        assert!(node.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs.as_strings().expect("should be string errors")[0]
            .contains("unknown function code"));
    }

    #[test]
    fn agg_expr_null_state() {
        let result = unsafe {
            ra_agg_expr(
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn window_expr_row_number() {
        let st = new_state();
        let alias = b"rn";
        let node = unsafe {
            ra_window_expr(
                st,
                5,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                alias.as_ptr().cast::<c_char>(),
                alias.len(),
            )
        };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Window);
        let state = unsafe { free_state(st) };
        let win = state.take_window_expr(idx).expect("should exist");
        assert_eq!(win.function, WindowFunction::RowNumber);
        assert!(win.arg.is_none());
        assert!(win.partition_by.is_empty());
        assert!(win.order_by.is_empty());
        assert_eq!(win.alias.as_deref(), Some("rn"));
    }

    #[test]
    fn window_expr_sum_with_partition_and_order() {
        let st = new_state();
        let arg = unsafe { ra_column(st, c"amount".as_ptr()) };

        let part_col = unsafe { ra_column(st, c"dept".as_ptr()) };
        let part_list = unsafe { ra_list_new(st) };
        let part_list = unsafe { ra_list_push(st, part_list, part_col) };

        let ord_col = unsafe { ra_column(st, c"date".as_ptr()) };
        let sk = unsafe { ra_sort_key(st, ord_col, 1, 0) };
        let ord_list = unsafe { ra_list_new(st) };
        let ord_list = unsafe { ra_list_push(st, ord_list, sk) };

        let node = unsafe { ra_window_expr(st, 1, arg, part_list, ord_list, std::ptr::null(), 0) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Window);
        let state = unsafe { free_state(st) };
        let win = state.take_window_expr(idx).expect("should exist");
        assert_eq!(win.function, WindowFunction::Sum);
        assert!(win.arg.is_some());
        assert_eq!(win.partition_by.len(), 1);
        assert_eq!(win.order_by.len(), 1);
        assert!(win.alias.is_none());
    }

    #[test]
    fn window_expr_all_functions() {
        let st = new_state();
        let expected = [
            (0_u32, WindowFunction::Avg),
            (1, WindowFunction::Sum),
            (2, WindowFunction::Count),
            (3, WindowFunction::Min),
            (4, WindowFunction::Max),
            (5, WindowFunction::RowNumber),
            (6, WindowFunction::Rank),
            (7, WindowFunction::DenseRank),
            (8, WindowFunction::PercentRank),
            (9, WindowFunction::Ntile),
            (10, WindowFunction::Lag),
            (11, WindowFunction::Lead),
            (12, WindowFunction::FirstValue),
            (13, WindowFunction::LastValue),
            (14, WindowFunction::NthValue),
        ];
        for (code, func) in expected {
            let node = unsafe {
                ra_window_expr(
                    st,
                    code,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0,
                )
            };
            assert!(!node.is_null());
            let (tag, idx) = decode(node).expect("should decode");
            assert_eq!(tag, NodeTag::Window);
            let win = unsafe { &*st }.take_window_expr(idx).expect("should exist");
            assert_eq!(win.function, func);
        }
        unsafe { free_state(st) };
    }

    #[test]
    fn window_expr_invalid_function_records_error() {
        let st = new_state();
        let node = unsafe {
            ra_window_expr(
                st,
                99,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        assert!(node.is_null());

        let state = unsafe { free_state(st) };
        let errs = state.take_result().expect_err("should have errors");
        assert!(errs.as_strings().expect("should be string errors")[0]
            .contains("unknown function code"));
    }

    #[test]
    fn window_expr_null_state() {
        let result = unsafe {
            ra_window_expr(
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        assert!(result.is_null());
    }

    // -----------------------------------------------------------------------
    // New builder tests: Priority 2 Expr builders
    // -----------------------------------------------------------------------

    #[test]
    fn array_builds_correctly() {
        let st = new_state();
        let v1 = unsafe { ra_const_int(st, 1) };
        let v2 = unsafe { ra_const_int(st, 2) };
        let v3 = unsafe { ra_const_int(st, 3) };
        let list = unsafe { ra_list_new(st) };
        let list = unsafe { ra_list_push(st, list, v1) };
        let list = unsafe { ra_list_push(st, list, v2) };
        let list = unsafe { ra_list_push(st, list, v3) };
        let node = unsafe { ra_array(st, list) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::Array(elements) = &expr {
            assert_eq!(elements.len(), 3);
        } else {
            panic!("expected Array variant");
        }
    }

    #[test]
    fn array_empty() {
        let st = new_state();
        let list = unsafe { ra_list_new(st) };
        let node = unsafe { ra_array(st, list) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::Array(elements) = &expr {
            assert!(elements.is_empty());
        } else {
            panic!("expected Array variant");
        }
    }

    #[test]
    fn array_null_state() {
        let result = unsafe { ra_array(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert!(result.is_null());
    }

    #[test]
    fn array_index_builds_correctly() {
        let st = new_state();
        let arr = unsafe { ra_column(st, c"arr".as_ptr()) };
        let idx_expr = unsafe { ra_const_int(st, 2) };
        let node = unsafe { ra_array_index(st, arr, idx_expr) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        assert!(matches!(expr, Expr::ArrayIndex(_, _)));
    }

    #[test]
    fn array_index_null_state() {
        let result = unsafe {
            ra_array_index(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(result.is_null());
    }

    #[test]
    fn field_access_builds_correctly() {
        let st = new_state();
        let base = unsafe { ra_column(st, c"row_val".as_ptr()) };
        let field = b"name";
        let node =
            unsafe { ra_field_access(st, base, field.as_ptr().cast::<c_char>(), field.len()) };
        assert!(!node.is_null());

        let (tag, idx) = decode(node).expect("should decode");
        assert_eq!(tag, NodeTag::Expr);
        let state = unsafe { free_state(st) };
        let expr = state.take_expr(idx).expect("should exist");
        if let Expr::FieldAccess { field_name, .. } = &expr {
            assert_eq!(field_name, "name");
        } else {
            panic!("expected FieldAccess variant");
        }
    }

    #[test]
    fn field_access_null_state() {
        let result = unsafe {
            ra_field_access(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
            )
        };
        assert!(result.is_null());
    }
}
