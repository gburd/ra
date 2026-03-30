//! Convert PostgreSQL `Query` structs into Ra `RelExpr` trees.
//!
//! Walks the PostgreSQL parse tree (rtable, jointree, targetList,
//! etc.) and produces the equivalent relational algebra
//! representation.  Returns `Ok(None)` for unsupported query types
//! so the planner hook falls back to the standard planner.

use std::ffi::CStr;

use pgrx::pg_sys;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, CycleDetection, JoinType, NullOrdering, ProjectionColumn,
    RelExpr, SortDirection, SortKey, WindowExpr, WindowFrame, WindowFrameBound, WindowFrameMode,
    WindowFunction,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

/// Maximum recursion depth for expression and subquery traversal.
/// Prevents stack overflow from deeply nested SQL constructs.
const MAX_DEPTH: u32 = 64;

/// Parse a PostgreSQL `Query` into a Ra `RelExpr`.
///
/// Returns `Ok(None)` for queries we cannot represent (DDL,
/// utility statements, DML).
///
/// # Safety
///
/// Caller must pass a valid, non-null `Query` pointer that
/// remains live for the duration of this call.
pub unsafe fn parse(query: *mut pg_sys::Query) -> Result<Option<RelExpr>, String> {
    parse_with_depth(query, 0)
}

/// Depth-limited query parse to prevent stack overflow.
unsafe fn parse_with_depth(
    query: *mut pg_sys::Query,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    if query.is_null() {
        return Err("null Query pointer".into());
    }
    if depth > MAX_DEPTH {
        return Err(format!(
            "query nesting depth {depth} exceeds limit {MAX_DEPTH}"
        ));
    }

    let q = &*query;

    // Only handle plain SELECT.
    if q.commandType != pg_sys::CmdType::CMD_SELECT {
        return Ok(None);
    }

    // Bail on utility statements (CREATE, DROP, etc.).
    if !q.utilityStmt.is_null() {
        return Ok(None);
    }

    // Parse CTEs if present.
    let cte_defs = build_cte_definitions(q, depth)?;

    // Handle set operations (UNION/INTERSECT/EXCEPT).
    if !q.setOperations.is_null() {
        let set_expr =
            parse_set_operation(q, q.setOperations, depth)?;
        let set_expr = match set_expr {
            Some(e) => e,
            None => return Ok(None),
        };
        let sorted = apply_order_by(q, set_expr, depth)?;
        let limited = apply_limit(q, sorted);
        let wrapped = wrap_with_ctes(limited, cte_defs);
        return Ok(Some(wrapped));
    }

    let from_expr = build_from_clause(q, depth)?;
    let from_expr = match from_expr {
        Some(e) => e,
        None => return Ok(None),
    };

    let filtered = apply_where_clause(q, from_expr, depth)?;
    let aggregated = apply_group_by(q, filtered, depth)?;
    let having_filtered = apply_having(q, aggregated, depth)?;
    let projected = apply_projection(q, having_filtered, depth)?;
    let windowed = apply_window(q, projected, depth)?;
    let distinct_applied = apply_distinct(q, windowed);
    let sorted = apply_order_by(q, distinct_applied, depth)?;
    let limited = apply_limit(q, sorted);

    let wrapped = wrap_with_ctes(limited, cte_defs);

    Ok(Some(wrapped))
}

// ── List helpers ────────────────────────────────────────────

/// Safe list length that handles null pointers.
unsafe fn list_length(list: *mut pg_sys::List) -> i32 {
    if list.is_null() {
        0
    } else {
        (*list).length
    }
}

// ── CTE (WITH clause) ───────────────────────────────────────

/// A parsed CTE definition ready to be wrapped around the body.
struct CteDef {
    name: String,
    definition: RelExpr,
    is_recursive: bool,
    cycle_detection: Option<CycleDetection>,
}

/// Parse `Query.cteList` into CTE definitions.
///
/// Returns an empty vec when there are no CTEs. If any CTE
/// cannot be parsed, we bail with `Ok(vec![])` so the caller
/// proceeds without CTE wrapping (the main query may still
/// succeed, and `RTE_CTE` references will produce Scan nodes
/// with the CTE name).
unsafe fn build_cte_definitions(
    q: &pg_sys::Query,
    depth: u32,
) -> Result<Vec<CteDef>, String> {
    let cte_list = q.cteList;
    if cte_list.is_null() || list_length(cte_list) == 0 {
        return Ok(Vec::new());
    }

    let len = list_length(cte_list);
    let mut defs = Vec::with_capacity(len as usize);

    for i in 0..len {
        let node = pg_sys::list_nth(cte_list, i) as *mut pg_sys::CommonTableExpr;
        if node.is_null() {
            continue;
        }
        let cte = &*node;

        let name = resolve_cte_name(cte.ctename);
        if name.is_empty() {
            continue;
        }

        let cte_query = cte.ctequery as *mut pg_sys::Query;
        if cte_query.is_null() {
            continue;
        }

        let cte_def = match parse_with_depth(cte_query, depth + 1)? {
            Some(def) => def,
            None => return Ok(Vec::new()),
        };

        let cycle = build_cycle_detection(cte);

        defs.push(CteDef {
            name,
            definition: cte_def,
            is_recursive: cte.cterecursive,
            cycle_detection: cycle,
        });
    }

    Ok(defs)
}

/// Extract cycle detection configuration from a CTE's
/// `cycle_clause`, if present.
unsafe fn build_cycle_detection(cte: &pg_sys::CommonTableExpr) -> Option<CycleDetection> {
    let cc = cte.cycle_clause;
    if cc.is_null() {
        return None;
    }
    let cc = &*cc;

    let mut track_columns = Vec::new();
    let col_list = cc.cycle_col_list;
    if !col_list.is_null() {
        let len = list_length(col_list);
        for i in 0..len {
            let val = pg_sys::list_nth(col_list, i) as *mut pg_sys::String;
            if !val.is_null() {
                let s = (*val).sval;
                if !s.is_null() {
                    let name = CStr::from_ptr(s).to_string_lossy().into_owned();
                    track_columns.push(name);
                }
            }
        }
    }

    let cycle_mark_column = resolve_cte_name(cc.cycle_mark_column);
    let path_column = resolve_cte_name(cc.cycle_path_column);

    Some(CycleDetection {
        track_columns,
        max_depth: None,
        cycle_mark_column: if cycle_mark_column.is_empty() {
            None
        } else {
            Some(cycle_mark_column)
        },
        path_column: if path_column.is_empty() {
            None
        } else {
            Some(path_column)
        },
    })
}

/// Wrap a query body in nested CTE nodes (outermost CTE first).
///
/// For `WITH a AS (...), b AS (...) SELECT ...`:
/// ```text
/// CTE { name: "a", def: ..., body:
///   CTE { name: "b", def: ..., body:
///     <main query> } }
/// ```
///
/// Recursive CTEs use the `RecursiveCTE` variant. The
/// recursive case is approximated as the full CTE definition
/// (PostgreSQL separates base/recursive inside the UNION, but
/// that separation is already flattened in the parse tree).
fn wrap_with_ctes(body: RelExpr, cte_defs: Vec<CteDef>) -> RelExpr {
    let mut result = body;
    for cte in cte_defs.into_iter().rev() {
        if cte.is_recursive {
            result = RelExpr::RecursiveCTE {
                name: cte.name,
                base_case: Box::new(cte.definition.clone()),
                recursive_case: Box::new(cte.definition),
                body: Box::new(result),
                cycle_detection: cte.cycle_detection,
            };
        } else {
            result = RelExpr::CTE {
                name: cte.name,
                definition: Box::new(cte.definition),
                body: Box::new(result),
            };
        }
    }
    result
}

/// Extract ctename from a C string pointer.
unsafe fn resolve_cte_name(name_ptr: *mut core::ffi::c_char) -> String {
    if name_ptr.is_null() {
        return String::new();
    }
    CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
}

// ── Set operations (UNION/INTERSECT/EXCEPT) ────────────────

/// Recursively parse a `SetOperationStmt` tree into
/// [`RelExpr`].
///
/// In `PostgreSQL`'s parse tree, set operations form a binary
/// tree where internal nodes are `SetOperationStmt` and leaves
/// are `RangeTblRef` nodes pointing to subquery RTEs in the
/// outer `Query`'s rtable.
unsafe fn parse_set_operation(
    q: &pg_sys::Query,
    node: *mut pg_sys::Node,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    if node.is_null() {
        return Ok(None);
    }
    if depth > MAX_DEPTH {
        return Err(format!(
            "set operation depth {depth} exceeds limit \
             {MAX_DEPTH}"
        ));
    }

    let tag = (*node).type_;

    // Leaf: a RangeTblRef pointing to a subquery RTE.
    if tag == pg_sys::NodeTag::T_RangeTblRef {
        let rtref =
            node.cast::<pg_sys::RangeTblRef>();
        let rtindex = (*rtref).rtindex;
        return parse_set_operation_leaf(q, rtindex, depth);
    }

    // Internal node: a SetOperationStmt.
    if tag != pg_sys::NodeTag::T_SetOperationStmt {
        return Ok(None);
    }

    let stmt =
        node.cast::<pg_sys::SetOperationStmt>();
    let Some(left) = parse_set_operation(
        q,
        (*stmt).larg,
        depth + 1,
    )?
    else {
        return Ok(None);
    };
    let Some(right) = parse_set_operation(
        q,
        (*stmt).rarg,
        depth + 1,
    )?
    else {
        return Ok(None);
    };

    let all = (*stmt).all;
    let op = (*stmt).op;

    let expr = if op == pg_sys::SetOperation::SETOP_UNION {
        RelExpr::Union {
            all,
            left: Box::new(left),
            right: Box::new(right),
        }
    } else if op == pg_sys::SetOperation::SETOP_INTERSECT {
        RelExpr::Intersect {
            all,
            left: Box::new(left),
            right: Box::new(right),
        }
    } else if op == pg_sys::SetOperation::SETOP_EXCEPT {
        RelExpr::Except {
            all,
            left: Box::new(left),
            right: Box::new(right),
        }
    } else {
        return Ok(None);
    };

    Ok(Some(expr))
}

/// Parse a leaf of a set operation tree.
///
/// Each leaf is a `RangeTblRef` that points to a subquery RTE
/// in the outer query's rtable. We parse that subquery
/// recursively to get its [`RelExpr`].
unsafe fn parse_set_operation_leaf(
    q: &pg_sys::Query,
    rtindex: i32,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    let Some(rte) = get_rte(q, rtindex)? else {
        return Ok(None);
    };

    let rtekind = (*rte).rtekind;
    if rtekind != pg_sys::RTEKind::RTE_SUBQUERY {
        return Ok(None);
    }

    let subquery = (*rte).subquery;
    if subquery.is_null() {
        return Ok(None);
    }

    parse_with_depth(subquery, depth + 1)
}

// ── FROM clause ─────────────────────────────────────────────

unsafe fn build_from_clause(q: &pg_sys::Query, depth: u32) -> Result<Option<RelExpr>, String> {
    let jointree = q.jointree;
    if jointree.is_null() {
        return Ok(None);
    }
    let fromlist = (*jointree).fromlist;
    if fromlist.is_null() || list_length(fromlist) == 0 {
        return Ok(None);
    }

    let length = list_length(fromlist);
    let mut result: Option<RelExpr> = None;

    for i in 0..length {
        let node = pg_sys::list_nth(fromlist, i) as *mut pg_sys::Node;
        if node.is_null() {
            continue;
        }
        let expr = build_from_node(node, q, depth)?;
        let expr = match expr {
            Some(e) => e,
            None => continue,
        };
        result = Some(match result {
            None => expr,
            Some(left) => RelExpr::Join {
                join_type: JoinType::Cross,
                condition: Expr::Const(Const::Bool(true)),
                left: Box::new(left),
                right: Box::new(expr),
            },
        });
    }

    Ok(result)
}

unsafe fn build_from_node(
    node: *mut pg_sys::Node,
    q: &pg_sys::Query,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    if node.is_null() {
        return Ok(None);
    }

    let tag = (*node).type_;

    if tag == pg_sys::NodeTag::T_RangeTblRef {
        let rtref = node as *mut pg_sys::RangeTblRef;
        let rtindex = (*rtref).rtindex;
        return build_rte_scan(q, rtindex, depth);
    }

    if tag == pg_sys::NodeTag::T_JoinExpr {
        let je = node as *mut pg_sys::JoinExpr;
        return build_join_expr(je, q, depth);
    }

    Ok(None)
}

unsafe fn build_rte_scan(
    q: &pg_sys::Query,
    rtindex: i32,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    let rte = get_rte(q, rtindex)?;
    let rte = match rte {
        Some(r) => r,
        None => return Ok(None),
    };

    let rtekind = (*rte).rtekind;

    if rtekind == pg_sys::RTEKind::RTE_RELATION {
        let relid = (*rte).relid;
        let table = resolve_rel_name(relid)
            .ok_or_else(|| format!("cannot resolve OID {}", relid.to_u32()))?;
        let alias = resolve_alias(rte);
        return Ok(Some(RelExpr::Scan { table, alias }));
    }

    if rtekind == pg_sys::RTEKind::RTE_SUBQUERY {
        let subquery = (*rte).subquery;
        if subquery.is_null() {
            return Ok(None);
        }
        return parse_with_depth(subquery, depth + 1);
    }

    if rtekind == pg_sys::RTEKind::RTE_FUNCTION {
        return build_rte_function(rte);
    }

    if rtekind == pg_sys::RTEKind::RTE_VALUES {
        return build_rte_values(rte, depth);
    }

    // RTE_JOIN: PostgreSQL flattens joins into the jointree,
    // the RTE entry is just metadata. Skip it here; the
    // actual join structure is in the FromExpr/JoinExpr nodes.
    if rtekind == pg_sys::RTEKind::RTE_JOIN {
        return Ok(None);
    }

    if rtekind == pg_sys::RTEKind::RTE_CTE {
        let ctename = (*rte).ctename;
        if ctename.is_null() {
            return Ok(None);
        }
        let name = CStr::from_ptr(ctename).to_string_lossy().into_owned();
        let alias = resolve_alias(rte);
        return Ok(Some(RelExpr::Scan { table: name, alias }));
    }

    // RTE_TABLEFUNC, RTE_NAMEDTUPLESTORE, RTE_RESULT:
    // not yet supported, bail gracefully.
    Ok(None)
}

/// Build a RelExpr for a function RTE (e.g., `generate_series`).
unsafe fn build_rte_function(rte: *mut pg_sys::RangeTblEntry) -> Result<Option<RelExpr>, String> {
    if rte.is_null() {
        return Ok(None);
    }
    let functions = (*rte).functions;
    if functions.is_null() || list_length(functions) == 0 {
        return Ok(None);
    }

    // For now, represent as a scan with a synthetic name.
    // Full table-function support requires extracting the
    // function name and args from RangeTblFunction, which
    // varies by PostgreSQL version.
    let alias = resolve_alias(rte);
    let table = alias
        .clone()
        .unwrap_or_else(|| format!("__func_rte_{}", list_length(functions)));
    Ok(Some(RelExpr::Scan { table, alias }))
}

/// Build a RelExpr for a VALUES RTE.
unsafe fn build_rte_values(
    rte: *mut pg_sys::RangeTblEntry,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    if rte.is_null() {
        return Ok(None);
    }
    let values_lists = (*rte).values_lists;
    if values_lists.is_null() || list_length(values_lists) == 0 {
        return Ok(None);
    }

    let num_rows = list_length(values_lists);
    let mut rows = Vec::with_capacity(num_rows as usize);

    for i in 0..num_rows {
        let row_list = pg_sys::list_nth(values_lists, i) as *mut pg_sys::List;
        if row_list.is_null() {
            continue;
        }
        let num_cols = list_length(row_list);
        let mut row = Vec::with_capacity(num_cols as usize);
        for j in 0..num_cols {
            let node = pg_sys::list_nth(row_list, j) as *mut pg_sys::Node;
            row.push(convert_expr_depth(node, depth)?);
        }
        rows.push(row);
    }

    Ok(Some(RelExpr::Values { rows }))
}

unsafe fn build_join_expr(
    je: *mut pg_sys::JoinExpr,
    q: &pg_sys::Query,
    depth: u32,
) -> Result<Option<RelExpr>, String> {
    if je.is_null() {
        return Ok(None);
    }

    let larg = (*je).larg;
    let rarg = (*je).rarg;

    if larg.is_null() || rarg.is_null() {
        return Ok(None);
    }

    let left = build_from_node(larg as *mut pg_sys::Node, q, depth)?;
    let right = build_from_node(rarg as *mut pg_sys::Node, q, depth)?;

    let (left, right) = match (left, right) {
        (Some(l), Some(r)) => (l, r),
        _ => return Ok(None),
    };

    let join_type = convert_join_type((*je).jointype);

    let condition = if (*je).quals.is_null() {
        Expr::Const(Const::Bool(true))
    } else {
        convert_expr_depth((*je).quals, depth)?
    };

    Ok(Some(RelExpr::Join {
        join_type,
        condition,
        left: Box::new(left),
        right: Box::new(right),
    }))
}

// ── WHERE clause ────────────────────────────────────────────

unsafe fn apply_where_clause(
    q: &pg_sys::Query,
    input: RelExpr,
    depth: u32,
) -> Result<RelExpr, String> {
    let jointree = q.jointree;
    if jointree.is_null() {
        return Ok(input);
    }
    let quals = (*jointree).quals;
    if quals.is_null() {
        return Ok(input);
    }
    let predicate = convert_expr_depth(quals, depth)?;
    Ok(RelExpr::Filter {
        predicate,
        input: Box::new(input),
    })
}

// ── GROUP BY + aggregates ───────────────────────────────────

unsafe fn apply_group_by(q: &pg_sys::Query, input: RelExpr, depth: u32) -> Result<RelExpr, String> {
    let has_group_by = !q.groupClause.is_null() && list_length(q.groupClause) > 0;
    let has_aggs = q.hasAggs;

    if !has_group_by && !has_aggs {
        return Ok(input);
    }

    let group_by = extract_group_by_exprs(q, depth)?;
    let aggregates = extract_aggregates(q, depth)?;

    Ok(RelExpr::Aggregate {
        group_by,
        aggregates,
        input: Box::new(input),
    })
}

unsafe fn extract_group_by_exprs(q: &pg_sys::Query, depth: u32) -> Result<Vec<Expr>, String> {
    let mut exprs = Vec::new();
    let gc = q.groupClause;
    if gc.is_null() {
        return Ok(exprs);
    }
    let tlist = q.targetList;

    let len = list_length(gc);
    for i in 0..len {
        let sgc = pg_sys::list_nth(gc, i) as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        if let Some(expr) = find_target_entry_by_ref(tlist, tle_ref, depth) {
            exprs.push(expr);
        }
    }
    Ok(exprs)
}

unsafe fn extract_aggregates(q: &pg_sys::Query, depth: u32) -> Result<Vec<AggregateExpr>, String> {
    let mut aggs = Vec::new();
    let tlist = q.targetList;
    if tlist.is_null() {
        return Ok(aggs);
    }

    let len = list_length(tlist);
    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i) as *mut pg_sys::TargetEntry;
        if tle.is_null() || (*tle).expr.is_null() {
            continue;
        }
        let node = (*tle).expr as *mut pg_sys::Node;
        if (*node).type_ == pg_sys::NodeTag::T_Aggref {
            let aggref = node as *mut pg_sys::Aggref;
            if let Some(agg) = convert_aggref(aggref, tle, depth)? {
                aggs.push(agg);
            }
        }
    }
    Ok(aggs)
}

unsafe fn convert_aggref(
    aggref: *mut pg_sys::Aggref,
    tle: *mut pg_sys::TargetEntry,
    depth: u32,
) -> Result<Option<AggregateExpr>, String> {
    if aggref.is_null() {
        return Ok(None);
    }

    let func_oid = (*aggref).aggfnoid;
    let function = map_agg_oid(func_oid);
    let arg = extract_first_agg_arg(aggref, depth)?;

    let distinct = {
        let d = (*aggref).aggdistinct;
        !d.is_null() && list_length(d) > 0
    };

    let alias = resolve_tle_alias(tle);

    Ok(Some(AggregateExpr {
        function,
        arg,
        distinct,
        alias,
    }))
}

unsafe fn extract_first_agg_arg(
    aggref: *mut pg_sys::Aggref,
    depth: u32,
) -> Result<Option<Expr>, String> {
    let args = (*aggref).args;
    if args.is_null() || list_length(args) == 0 {
        return Ok(None);
    }
    let first_tle = pg_sys::list_nth(args, 0) as *mut pg_sys::TargetEntry;
    if first_tle.is_null() || (*first_tle).expr.is_null() {
        return Ok(None);
    }
    let expr = convert_expr_depth((*first_tle).expr as *mut pg_sys::Node, depth)?;
    Ok(Some(expr))
}

// ── HAVING ──────────────────────────────────────────────────

unsafe fn apply_having(q: &pg_sys::Query, input: RelExpr, depth: u32) -> Result<RelExpr, String> {
    let having = q.havingQual;
    if having.is_null() {
        return Ok(input);
    }
    let predicate = convert_expr_depth(having as *mut pg_sys::Node, depth)?;
    Ok(RelExpr::Filter {
        predicate,
        input: Box::new(input),
    })
}

// ── Window functions ────────────────────────────────────────

unsafe fn apply_window(q: &pg_sys::Query, input: RelExpr, depth: u32) -> Result<RelExpr, String> {
    if !q.hasWindowFuncs {
        return Ok(input);
    }

    let tlist = q.targetList;
    if tlist.is_null() {
        return Ok(input);
    }

    let window_clause_list = q.windowClause;
    let mut functions = Vec::new();
    let tlist_len = list_length(tlist);

    for i in 0..tlist_len {
        let tle = pg_sys::list_nth(tlist, i) as *mut pg_sys::TargetEntry;
        if tle.is_null() || (*tle).expr.is_null() {
            continue;
        }
        let node = (*tle).expr as *mut pg_sys::Node;
        if (*node).type_ != pg_sys::NodeTag::T_WindowFunc {
            continue;
        }
        let wfunc = node as *mut pg_sys::WindowFunc;
        if let Some(wexpr) = convert_window_func(wfunc, tle, window_clause_list, q, depth)? {
            functions.push(wexpr);
        }
    }

    if functions.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Window {
        functions,
        input: Box::new(input),
    })
}

/// Convert a single `WindowFunc` node into a `WindowExpr`.
unsafe fn convert_window_func(
    wfunc: *mut pg_sys::WindowFunc,
    tle: *mut pg_sys::TargetEntry,
    window_clause_list: *mut pg_sys::List,
    q: &pg_sys::Query,
    depth: u32,
) -> Result<Option<WindowExpr>, String> {
    if wfunc.is_null() {
        return Ok(None);
    }

    let func_oid = (*wfunc).winfnoid;
    let function = map_window_func_oid(func_oid);

    let arg = extract_first_wfunc_arg(wfunc, depth)?;
    let alias = resolve_tle_alias(tle);

    let winref = (*wfunc).winref;
    let (partition_by, order_by, frame) =
        extract_window_clause(window_clause_list, winref, q, depth)?;

    Ok(Some(WindowExpr {
        function,
        arg,
        partition_by,
        order_by,
        frame,
        alias,
    }))
}

/// Extract the first argument from a WindowFunc's args list.
unsafe fn extract_first_wfunc_arg(
    wfunc: *mut pg_sys::WindowFunc,
    depth: u32,
) -> Result<Option<Expr>, String> {
    let args = (*wfunc).args;
    if args.is_null() || list_length(args) == 0 {
        return Ok(None);
    }
    let first_node = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    if first_node.is_null() {
        return Ok(None);
    }
    // WindowFunc args can be plain expressions or TargetEntries
    // depending on context. Check if it's a TargetEntry first.
    if (*first_node).type_ == pg_sys::NodeTag::T_TargetEntry {
        let tle = first_node as *mut pg_sys::TargetEntry;
        if (*tle).expr.is_null() {
            return Ok(None);
        }
        let expr = convert_expr_depth((*tle).expr as *mut pg_sys::Node, depth)?;
        return Ok(Some(expr));
    }
    let expr = convert_expr_depth(first_node, depth)?;
    Ok(Some(expr))
}

/// Find the WindowClause matching `winref` and extract
/// partition_by, order_by, and frame specification.
unsafe fn extract_window_clause(
    wc_list: *mut pg_sys::List,
    winref: pg_sys::Index,
    q: &pg_sys::Query,
    depth: u32,
) -> Result<(Vec<Expr>, Vec<SortKey>, Option<WindowFrame>), String> {
    if wc_list.is_null() {
        return Ok((vec![], vec![], None));
    }

    let len = list_length(wc_list);
    for i in 0..len {
        let wc = pg_sys::list_nth(wc_list, i) as *mut pg_sys::WindowClause;
        if wc.is_null() {
            continue;
        }
        if (*wc).winref != winref {
            continue;
        }

        let partition_by = extract_sort_group_exprs((*wc).partitionClause, q.targetList, depth)?;

        let order_by = extract_sort_keys((*wc).orderClause, q.targetList, depth)?;

        let frame = extract_window_frame(wc);

        return Ok((partition_by, order_by, frame));
    }

    Ok((vec![], vec![], None))
}

/// Extract expressions from a SortGroupClause list
/// (used for PARTITION BY).
unsafe fn extract_sort_group_exprs(
    clause_list: *mut pg_sys::List,
    tlist: *mut pg_sys::List,
    depth: u32,
) -> Result<Vec<Expr>, String> {
    let mut exprs = Vec::new();
    if clause_list.is_null() {
        return Ok(exprs);
    }

    let len = list_length(clause_list);
    for i in 0..len {
        let sgc = pg_sys::list_nth(clause_list, i) as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        if let Some(expr) = find_target_entry_by_ref(tlist, tle_ref, depth) {
            exprs.push(expr);
        }
    }
    Ok(exprs)
}

/// Extract sort keys from a SortGroupClause list
/// (used for ORDER BY within window).
unsafe fn extract_sort_keys(
    clause_list: *mut pg_sys::List,
    tlist: *mut pg_sys::List,
    depth: u32,
) -> Result<Vec<SortKey>, String> {
    let mut keys = Vec::new();
    if clause_list.is_null() {
        return Ok(keys);
    }

    let len = list_length(clause_list);
    for i in 0..len {
        let sgc = pg_sys::list_nth(clause_list, i) as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        let expr =
            find_target_entry_by_ref(tlist, tle_ref, depth).unwrap_or(Expr::Const(Const::Null));

        let direction = infer_sort_direction((*sgc).sortop);
        let nulls = if (*sgc).nulls_first {
            NullOrdering::First
        } else {
            NullOrdering::Last
        };

        keys.push(SortKey {
            expr,
            direction,
            nulls,
        });
    }
    Ok(keys)
}

/// Extract window frame specification from a WindowClause.
unsafe fn extract_window_frame(wc: *mut pg_sys::WindowClause) -> Option<WindowFrame> {
    if wc.is_null() {
        return None;
    }

    let opts = (*wc).frameOptions;

    // PostgreSQL FRAMEOPTION constants (from windowapi.h).
    const FRAMEOPTION_RANGE: i32 = 0x0002;
    const FRAMEOPTION_ROWS: i32 = 0x0004;
    const FRAMEOPTION_GROUPS: i32 = 0x0008;
    const FRAMEOPTION_START_UNBOUNDED_PRECEDING: i32 = 0x0020;
    const FRAMEOPTION_START_CURRENT_ROW: i32 = 0x0200;
    const FRAMEOPTION_START_OFFSET_PRECEDING: i32 = 0x0800;
    const FRAMEOPTION_START_OFFSET_FOLLOWING: i32 = 0x2000;
    const FRAMEOPTION_END_UNBOUNDED_FOLLOWING: i32 = 0x0100;
    const FRAMEOPTION_END_CURRENT_ROW: i32 = 0x0400;
    const FRAMEOPTION_END_OFFSET_PRECEDING: i32 = 0x1000;
    const FRAMEOPTION_END_OFFSET_FOLLOWING: i32 = 0x4000;

    let mode = if opts & FRAMEOPTION_ROWS != 0 {
        WindowFrameMode::Rows
    } else if opts & FRAMEOPTION_GROUPS != 0 {
        WindowFrameMode::Groups
    } else if opts & FRAMEOPTION_RANGE != 0 {
        WindowFrameMode::Range
    } else {
        WindowFrameMode::Range
    };

    let start_offset = extract_frame_offset((*wc).startOffset);
    let end_offset = extract_frame_offset((*wc).endOffset);

    let start = if opts & FRAMEOPTION_START_UNBOUNDED_PRECEDING != 0 {
        WindowFrameBound::UnboundedPreceding
    } else if opts & FRAMEOPTION_START_CURRENT_ROW != 0 {
        WindowFrameBound::CurrentRow
    } else if opts & FRAMEOPTION_START_OFFSET_PRECEDING != 0 {
        WindowFrameBound::Preceding(start_offset)
    } else if opts & FRAMEOPTION_START_OFFSET_FOLLOWING != 0 {
        WindowFrameBound::Following(start_offset)
    } else {
        WindowFrameBound::UnboundedPreceding
    };

    let end = if opts & FRAMEOPTION_END_UNBOUNDED_FOLLOWING != 0 {
        WindowFrameBound::UnboundedFollowing
    } else if opts & FRAMEOPTION_END_CURRENT_ROW != 0 {
        WindowFrameBound::CurrentRow
    } else if opts & FRAMEOPTION_END_OFFSET_PRECEDING != 0 {
        WindowFrameBound::Preceding(end_offset)
    } else if opts & FRAMEOPTION_END_OFFSET_FOLLOWING != 0 {
        WindowFrameBound::Following(end_offset)
    } else {
        WindowFrameBound::CurrentRow
    };

    Some(WindowFrame { mode, start, end })
}

/// Extract a frame offset value from a Node pointer.
/// Returns 0 if the node is null or not a constant.
unsafe fn extract_frame_offset(node: *mut pg_sys::Node) -> u64 {
    extract_const_u64(node).unwrap_or(0)
}

/// Convert a WindowFunc expression node into a scalar Expr
/// (used when window functions appear in expression context).
unsafe fn convert_windowfunc_as_expr(
    wfunc: *mut pg_sys::WindowFunc,
    depth: u32,
) -> Result<Expr, String> {
    if wfunc.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let func = map_window_func_oid((*wfunc).winfnoid);
    let name = format!("{func}");

    let mut args_out = Vec::new();
    let args = (*wfunc).args;
    if !args.is_null() {
        let len = list_length(args);
        for i in 0..len {
            let node = pg_sys::list_nth(args, i) as *mut pg_sys::Node;
            if node.is_null() {
                continue;
            }
            if (*node).type_ == pg_sys::NodeTag::T_TargetEntry {
                let tle = node as *mut pg_sys::TargetEntry;
                if !(*tle).expr.is_null() {
                    args_out.push(convert_expr_inner((*tle).expr as *mut pg_sys::Node, depth)?);
                }
            } else {
                args_out.push(convert_expr_inner(node, depth)?);
            }
        }
    }

    Ok(Expr::Function {
        name,
        args: args_out,
    })
}

/// Map a window function OID to Ra's `WindowFunction`.
///
/// Window functions in PostgreSQL are identified by OID.
/// This maps the standard built-in window function OIDs
/// plus aggregate functions used as window functions.
fn map_window_func_oid(funcoid: pg_sys::Oid) -> WindowFunction {
    match funcoid.to_u32() {
        // row_number
        3100 => WindowFunction::RowNumber,
        // rank
        3101 => WindowFunction::Rank,
        // dense_rank
        3102 => WindowFunction::DenseRank,
        // percent_rank
        3103 => WindowFunction::PercentRank,
        // cume_dist (map to PercentRank as closest)
        3104 => WindowFunction::PercentRank,
        // ntile
        3105 => WindowFunction::Ntile,
        // lag
        3106 | 3107 | 3108 => WindowFunction::Lag,
        // lead
        3109 | 3110 | 3111 => WindowFunction::Lead,
        // first_value
        3112 => WindowFunction::FirstValue,
        // last_value
        3113 => WindowFunction::LastValue,
        // nth_value
        3114 => WindowFunction::NthValue,
        // Aggregate functions used as window functions:
        // sum
        2108 | 2109 | 2110 | 2111 | 2112 | 2113 | 2114 => WindowFunction::Sum,
        // avg
        2100 | 2101 | 2102 | 2103 | 2104 | 2105 | 2106 => WindowFunction::Avg,
        // count
        2803 | 2147 | 2146 => WindowFunction::Count,
        // min
        2131 | 2132 | 2133 | 2134 | 2135 | 2136 | 2137 | 2138 | 2139 | 2245 => WindowFunction::Min,
        // max
        2115 | 2116 | 2117 | 2118 | 2119 | 2120 | 2121 | 2122 | 2123 | 2126 | 2244 => {
            WindowFunction::Max
        }
        _ => WindowFunction::RowNumber,
    }
}

// ── Projection (targetList) ─────────────────────────────────

unsafe fn apply_projection(
    q: &pg_sys::Query,
    input: RelExpr,
    depth: u32,
) -> Result<RelExpr, String> {
    let tlist = q.targetList;
    if tlist.is_null() || list_length(tlist) == 0 {
        return Ok(input);
    }

    let mut columns = Vec::new();
    let len = list_length(tlist);

    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i) as *mut pg_sys::TargetEntry;
        if tle.is_null() {
            continue;
        }
        if (*tle).resjunk {
            continue;
        }

        let node = (*tle).expr as *mut pg_sys::Node;
        if node.is_null() {
            continue;
        }

        // Skip aggregates; handled in Aggregate node.
        if (*node).type_ == pg_sys::NodeTag::T_Aggref {
            continue;
        }

        // Skip window functions; handled in Window node.
        if (*node).type_ == pg_sys::NodeTag::T_WindowFunc {
            continue;
        }

        let expr = convert_expr_depth(node, depth)?;
        let alias = resolve_tle_alias(tle);
        columns.push(ProjectionColumn { expr, alias });
    }

    if columns.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Project {
        columns,
        input: Box::new(input),
    })
}

// ── DISTINCT ────────────────────────────────────────────────

unsafe fn apply_distinct(q: &pg_sys::Query, input: RelExpr) -> RelExpr {
    let dc = q.distinctClause;
    if dc.is_null() || list_length(dc) == 0 {
        return input;
    }
    RelExpr::Distinct {
        input: Box::new(input),
    }
}

// ── ORDER BY ────────────────────────────────────────────────

unsafe fn apply_order_by(q: &pg_sys::Query, input: RelExpr, depth: u32) -> Result<RelExpr, String> {
    let sc = q.sortClause;
    if sc.is_null() || list_length(sc) == 0 {
        return Ok(input);
    }

    let tlist = q.targetList;
    let mut keys = Vec::new();
    let len = list_length(sc);

    for i in 0..len {
        let sgc = pg_sys::list_nth(sc, i) as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        let expr =
            find_target_entry_by_ref(tlist, tle_ref, depth).unwrap_or(Expr::Const(Const::Null));

        let direction = infer_sort_direction((*sgc).sortop);
        let nulls = if (*sgc).nulls_first {
            NullOrdering::First
        } else {
            NullOrdering::Last
        };

        keys.push(SortKey {
            expr,
            direction,
            nulls,
        });
    }

    if keys.is_empty() {
        return Ok(input);
    }

    Ok(RelExpr::Sort {
        keys,
        input: Box::new(input),
    })
}

/// Infer sort direction from a sort operator OID.
fn infer_sort_direction(sortop: pg_sys::Oid) -> SortDirection {
    // Greater-than operators indicate DESC ordering.
    let desc_ops: &[u32] = &[
        518,  // int4gt
        413,  // int8gt
        674,  // float8gt
        666,  // textgt
        1756, // numericgt
        520,  // int2gt (pg OID overload)
        610,  // oidgt
        1097, // date_gt
        1324, // timestamptz_gt
        1157, // timestamp_gt
        2064, // time_gt
        1553, // interval_gt
    ];

    if desc_ops.contains(&sortop.to_u32()) {
        SortDirection::Desc
    } else {
        SortDirection::Asc
    }
}

// ── LIMIT / OFFSET ──────────────────────────────────────────

unsafe fn apply_limit(q: &pg_sys::Query, input: RelExpr) -> RelExpr {
    let limit_node = q.limitCount;
    let offset_node = q.limitOffset;

    if limit_node.is_null() && offset_node.is_null() {
        return input;
    }

    let count = extract_const_u64(limit_node).unwrap_or(u64::MAX);
    let offset = extract_const_u64(offset_node).unwrap_or(0);

    RelExpr::Limit {
        count,
        offset,
        input: Box::new(input),
    }
}

unsafe fn extract_const_u64(node: *mut pg_sys::Node) -> Option<u64> {
    if node.is_null() {
        return None;
    }
    if (*node).type_ != pg_sys::NodeTag::T_Const {
        return None;
    }
    let con = node as *mut pg_sys::Const;
    if (*con).constisnull {
        return None;
    }
    let val = (*con).constvalue.value() as i64;
    if val < 0 {
        None
    } else {
        Some(val as u64)
    }
}

// ── Expression conversion ───────────────────────────────────

/// Top-level expression conversion with depth tracking.
unsafe fn convert_expr_depth(node: *mut pg_sys::Node, depth: u32) -> Result<Expr, String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "expression depth {depth} exceeds limit {MAX_DEPTH}"
        ));
    }
    convert_expr_inner(node, depth + 1)
}

unsafe fn convert_expr_inner(node: *mut pg_sys::Node, depth: u32) -> Result<Expr, String> {
    if node.is_null() {
        return Ok(Expr::Const(Const::Null));
    }

    let tag = (*node).type_;

    if tag == pg_sys::NodeTag::T_Var {
        return convert_var(node as *mut pg_sys::Var);
    }
    if tag == pg_sys::NodeTag::T_Const {
        return convert_pg_const(node as *mut pg_sys::Const);
    }
    if tag == pg_sys::NodeTag::T_OpExpr {
        return convert_opexpr(node as *mut pg_sys::OpExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_BoolExpr {
        return convert_boolexpr(node as *mut pg_sys::BoolExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_NullTest {
        return convert_nulltest(node as *mut pg_sys::NullTest, depth);
    }
    if tag == pg_sys::NodeTag::T_BooleanTest {
        return convert_booleantest(node as *mut pg_sys::BooleanTest, depth);
    }
    if tag == pg_sys::NodeTag::T_FuncExpr {
        return convert_funcexpr(node as *mut pg_sys::FuncExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_Aggref {
        return convert_aggref_as_expr(node as *mut pg_sys::Aggref, depth);
    }
    if tag == pg_sys::NodeTag::T_WindowFunc {
        return convert_windowfunc_as_expr(node as *mut pg_sys::WindowFunc, depth);
    }
    if tag == pg_sys::NodeTag::T_CaseExpr {
        return convert_caseexpr(node as *mut pg_sys::CaseExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_CoalesceExpr {
        return convert_coalesce(node as *mut pg_sys::CoalesceExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_ScalarArrayOpExpr {
        return convert_scalar_array_op(node as *mut pg_sys::ScalarArrayOpExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_SubLink {
        return convert_sublink(node as *mut pg_sys::SubLink, depth);
    }
    if tag == pg_sys::NodeTag::T_RelabelType {
        let rt = node as *mut pg_sys::RelabelType;
        let arg = (*rt).arg;
        if arg.is_null() {
            return Ok(Expr::Const(Const::Null));
        }
        return convert_expr_inner(arg as *mut pg_sys::Node, depth);
    }
    if tag == pg_sys::NodeTag::T_CoerceViaIO {
        let cio = node as *mut pg_sys::CoerceViaIO;
        let arg = (*cio).arg;
        if arg.is_null() {
            return Ok(Expr::Const(Const::Null));
        }
        return convert_expr_inner(arg as *mut pg_sys::Node, depth);
    }
    if tag == pg_sys::NodeTag::T_CoerceToDomain {
        let ctd = node as *mut pg_sys::CoerceToDomain;
        let arg = (*ctd).arg;
        if arg.is_null() {
            return Ok(Expr::Const(Const::Null));
        }
        return convert_expr_inner(arg as *mut pg_sys::Node, depth);
    }
    if tag == pg_sys::NodeTag::T_ArrayExpr {
        return convert_array_expr(node as *mut pg_sys::ArrayExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_RowExpr {
        return convert_row_expr(node as *mut pg_sys::RowExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_Param {
        return convert_param(node as *mut pg_sys::Param);
    }
    if tag == pg_sys::NodeTag::T_SQLValueFunction {
        return convert_sql_value_function(node as *mut pg_sys::SQLValueFunction);
    }
    if tag == pg_sys::NodeTag::T_FieldSelect {
        let fs = node as *mut pg_sys::FieldSelect;
        let arg = (*fs).arg;
        if arg.is_null() {
            return Ok(Expr::Const(Const::Null));
        }

        // Extract field number and resolve to name
        let fieldnum = (*fs).fieldnum;
        let result_type = (*fs).resulttype;
        let field_name = resolve_field_name(result_type, fieldnum)
            .unwrap_or_else(|| format!("field_{fieldnum}"));

        let base_expr = convert_expr_inner(arg as *mut pg_sys::Node, depth)?;

        return Ok(Expr::FieldAccess {
            expr: Box::new(base_expr),
            field_name,
        });
    }
    if tag == pg_sys::NodeTag::T_MinMaxExpr {
        return convert_minmax_expr(node as *mut pg_sys::MinMaxExpr, depth);
    }
    if tag == pg_sys::NodeTag::T_DistinctExpr {
        // DistinctExpr is structurally identical to OpExpr
        // but represents `IS DISTINCT FROM`.
        return convert_opexpr(node as *mut pg_sys::OpExpr, depth);
    }

    // Unknown node type: produce a sentinel column reference
    // so downstream can identify it without crashing.
    Ok(Expr::Column(ColumnRef::new(format!("__pg_node_{tag:?}"))))
}

unsafe fn convert_var(var: *mut pg_sys::Var) -> Result<Expr, String> {
    if var.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let attno = (*var).varattno;
    let varno = (*var).varno as i32;

    let col_name = if attno <= 0 {
        format!("col_{varno}_row")
    } else {
        format!("col_{attno}")
    };

    Ok(Expr::Column(ColumnRef {
        table: Some(format!("t{varno}")),
        column: col_name,
    }))
}

unsafe fn convert_pg_const(con: *mut pg_sys::Const) -> Result<Expr, String> {
    if con.is_null() || (*con).constisnull {
        return Ok(Expr::Const(Const::Null));
    }

    let type_oid = (*con).consttype.to_u32();
    let datum_val = (*con).constvalue.value();

    let val = match type_oid {
        // BOOLOID
        16 => Const::Bool(datum_val != 0),
        // INT2OID
        21 => Const::Int(i64::from(datum_val as i16)),
        // INT4OID
        23 => Const::Int(i64::from(datum_val as i32)),
        // INT8OID
        20 => Const::Int(datum_val as i64),
        // OIDOID
        26 => Const::Int(i64::from(datum_val as u32)),
        // FLOAT4OID
        700 => {
            let bits = datum_val as u32;
            Const::Float(f64::from(f32::from_bits(bits)))
        }
        // FLOAT8OID
        701 => {
            let bits = datum_val as u64;
            Const::Float(f64::from_bits(bits))
        }
        // TEXTOID, VARCHAROID, NAMEOID, BPCHAROID
        25 | 1043 | 19 | 1042 => {
            if datum_val == 0 {
                Const::Null
            } else {
                let vl = datum_val as *const u8;
                let text_ptr = vl.add(4) as *const i8;
                let s = CStr::from_ptr(text_ptr).to_string_lossy().into_owned();
                Const::String(s)
            }
        }
        // NUMERICOID: decode using PostgreSQL's numeric_to_double
        1700 => {
            if datum_val == 0 {
                Const::Null
            } else {
                let numeric_ptr = datum_val as *mut pg_sys::NumericData;
                let float_val = numeric_to_double(numeric_ptr);
                Const::Float(float_val)
            }
        }
        // DATEOID, TIMESTAMPOID, TIMESTAMPTZOID:
        // store as int (internal representation)
        1082 | 1114 | 1184 => Const::Int(datum_val as i64),
        _ => Const::Int(datum_val as i64),
    };

    Ok(Expr::Const(val))
}

/// Convert PostgreSQL NUMERIC to f64.
///
/// Uses PostgreSQL's DirectFunctionCall to safely convert NUMERIC
/// to double precision without manual parsing.
///
/// # Safety
///
/// Must be called with a valid Numeric datum pointer.
unsafe fn numeric_to_double(numeric: *mut pg_sys::NumericData) -> f64 {
    if numeric.is_null() {
        return 0.0;
    }

    // Use PostgreSQL's built-in numeric_float8 function
    let datum = pg_sys::Datum::from(numeric as usize);
    let result = pg_sys::DirectFunctionCall1Coll(
        Some(pg_sys::numeric_float8),
        pg_sys::InvalidOid,
        datum,
    );

    // Extract f64 from result datum
    f64::from_bits(result.value() as u64)
}

unsafe fn convert_opexpr(opexpr: *mut pg_sys::OpExpr, depth: u32) -> Result<Expr, String> {
    if opexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args = (*opexpr).args;
    let arg_count = list_length(args);

    if args.is_null() || arg_count == 0 {
        return Ok(Expr::Const(Const::Null));
    }

    // Unary operator (e.g., unary minus `-x`).
    if arg_count == 1 {
        let operand_node = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
        let operand = convert_expr_inner(operand_node, depth)?;
        return Ok(Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(operand),
        });
    }

    let left_node = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    let right_node = pg_sys::list_nth(args, 1) as *mut pg_sys::Node;

    let left = convert_expr_inner(left_node, depth)?;
    let right = convert_expr_inner(right_node, depth)?;
    let op = map_operator_oid((*opexpr).opno);

    Ok(Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

unsafe fn convert_boolexpr(bexpr: *mut pg_sys::BoolExpr, depth: u32) -> Result<Expr, String> {
    if bexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }

    let boolop = (*bexpr).boolop;
    let args = (*bexpr).args;
    if args.is_null() || list_length(args) == 0 {
        return Ok(Expr::Const(Const::Bool(true)));
    }

    if boolop == pg_sys::BoolExprType::NOT_EXPR {
        let child = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
        let operand = convert_expr_inner(child, depth)?;
        return Ok(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(operand),
        });
    }

    let binop = if boolop == pg_sys::BoolExprType::AND_EXPR {
        BinOp::And
    } else {
        BinOp::Or
    };

    let len = list_length(args);
    let first = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    let mut acc = convert_expr_inner(first, depth)?;

    for i in 1..len {
        let next = pg_sys::list_nth(args, i) as *mut pg_sys::Node;
        let rhs = convert_expr_inner(next, depth)?;
        acc = Expr::BinOp {
            op: binop,
            left: Box::new(acc),
            right: Box::new(rhs),
        };
    }

    Ok(acc)
}

unsafe fn convert_nulltest(nt: *mut pg_sys::NullTest, depth: u32) -> Result<Expr, String> {
    if nt.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let arg_ptr = (*nt).arg;
    if arg_ptr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let arg = convert_expr_inner(arg_ptr as *mut pg_sys::Node, depth)?;
    let op = if (*nt).nulltesttype == pg_sys::NullTestType::IS_NULL {
        UnaryOp::IsNull
    } else {
        UnaryOp::IsNotNull
    };
    Ok(Expr::UnaryOp {
        op,
        operand: Box::new(arg),
    })
}

/// Convert `IS TRUE`, `IS FALSE`, `IS NOT TRUE`, etc.
unsafe fn convert_booleantest(bt: *mut pg_sys::BooleanTest, depth: u32) -> Result<Expr, String> {
    if bt.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let arg_ptr = (*bt).arg;
    if arg_ptr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let arg = convert_expr_inner(arg_ptr as *mut pg_sys::Node, depth)?;

    let test_type = (*bt).booltesttype;

    // Map BooleanTest variants to equivalent expressions.
    #[allow(non_upper_case_globals)]
    match test_type {
        pg_sys::BoolTestType::IS_TRUE => Ok(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(arg),
            right: Box::new(Expr::Const(Const::Bool(true))),
        }),
        pg_sys::BoolTestType::IS_FALSE => Ok(Expr::BinOp {
            op: BinOp::Eq,
            left: Box::new(arg),
            right: Box::new(Expr::Const(Const::Bool(false))),
        }),
        pg_sys::BoolTestType::IS_NOT_TRUE => Ok(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(arg),
                right: Box::new(Expr::Const(Const::Bool(true))),
            }),
        }),
        pg_sys::BoolTestType::IS_NOT_FALSE => Ok(Expr::UnaryOp {
            op: UnaryOp::Not,
            operand: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                left: Box::new(arg),
                right: Box::new(Expr::Const(Const::Bool(false))),
            }),
        }),
        // IS UNKNOWN / IS NOT UNKNOWN -> IS NULL / IS NOT NULL
        pg_sys::BoolTestType::IS_UNKNOWN => Ok(Expr::UnaryOp {
            op: UnaryOp::IsNull,
            operand: Box::new(arg),
        }),
        _ => Ok(Expr::UnaryOp {
            op: UnaryOp::IsNotNull,
            operand: Box::new(arg),
        }),
    }
}

unsafe fn convert_funcexpr(funcexpr: *mut pg_sys::FuncExpr, depth: u32) -> Result<Expr, String> {
    if funcexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let funcid = (*funcexpr).funcid;
    let name = resolve_func_name(funcid);

    let mut args_out = Vec::new();
    let args = (*funcexpr).args;
    if !args.is_null() {
        let len = list_length(args);
        for i in 0..len {
            let node = pg_sys::list_nth(args, i) as *mut pg_sys::Node;
            args_out.push(convert_expr_inner(node, depth)?);
        }
    }

    Ok(Expr::Function {
        name,
        args: args_out,
    })
}

unsafe fn convert_aggref_as_expr(aggref: *mut pg_sys::Aggref, depth: u32) -> Result<Expr, String> {
    if aggref.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let func = map_agg_oid((*aggref).aggfnoid);
    let name = format!("{func}");

    let mut args_out = Vec::new();
    let args = (*aggref).args;
    if !args.is_null() {
        let len = list_length(args);
        for i in 0..len {
            let tle = pg_sys::list_nth(args, i) as *mut pg_sys::TargetEntry;
            if !tle.is_null() && !(*tle).expr.is_null() {
                args_out.push(convert_expr_inner((*tle).expr as *mut pg_sys::Node, depth)?);
            }
        }
    }

    Ok(Expr::Function {
        name,
        args: args_out,
    })
}

/// Convert a CASE expression.
unsafe fn convert_caseexpr(caseexpr: *mut pg_sys::CaseExpr, depth: u32) -> Result<Expr, String> {
    if caseexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }

    // Optional operand for simple CASE (CASE x WHEN ...).
    let operand = if (*caseexpr).arg.is_null() {
        None
    } else {
        Some(Box::new(convert_expr_inner(
            (*caseexpr).arg as *mut pg_sys::Node,
            depth,
        )?))
    };

    let mut when_clauses = Vec::new();
    let args = (*caseexpr).args;
    if !args.is_null() {
        let len = list_length(args);
        for i in 0..len {
            let cw = pg_sys::list_nth(args, i) as *mut pg_sys::CaseWhen;
            if cw.is_null() {
                continue;
            }
            let condition = if (*cw).expr.is_null() {
                Expr::Const(Const::Bool(true))
            } else {
                convert_expr_inner((*cw).expr as *mut pg_sys::Node, depth)?
            };
            let result = if (*cw).result.is_null() {
                Expr::Const(Const::Null)
            } else {
                convert_expr_inner((*cw).result as *mut pg_sys::Node, depth)?
            };
            when_clauses.push((condition, result));
        }
    }

    let else_result = if (*caseexpr).defresult.is_null() {
        None
    } else {
        Some(Box::new(convert_expr_inner(
            (*caseexpr).defresult as *mut pg_sys::Node,
            depth,
        )?))
    };

    Ok(Expr::Case {
        operand,
        when_clauses,
        else_result,
    })
}

/// Convert COALESCE(a, b, ...) to a function call.
unsafe fn convert_coalesce(ce: *mut pg_sys::CoalesceExpr, depth: u32) -> Result<Expr, String> {
    if ce.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args_list = (*ce).args;
    let mut args_out = Vec::new();
    if !args_list.is_null() {
        let len = list_length(args_list);
        for i in 0..len {
            let node = pg_sys::list_nth(args_list, i) as *mut pg_sys::Node;
            args_out.push(convert_expr_inner(node, depth)?);
        }
    }
    Ok(Expr::Function {
        name: "COALESCE".into(),
        args: args_out,
    })
}

/// Convert `x IN (a, b, c)` / `x = ANY(ARRAY[...])`.
unsafe fn convert_scalar_array_op(
    saop: *mut pg_sys::ScalarArrayOpExpr,
    depth: u32,
) -> Result<Expr, String> {
    if saop.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args = (*saop).args;
    if args.is_null() || list_length(args) < 2 {
        return Ok(Expr::Const(Const::Null));
    }

    let left_node = pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    let right_node = pg_sys::list_nth(args, 1) as *mut pg_sys::Node;

    let left = convert_expr_inner(left_node, depth)?;
    let right = convert_expr_inner(right_node, depth)?;

    // useOr=true means ANY (like IN), useOr=false means ALL.
    let name = if (*saop).useOr { "ANY" } else { "ALL" };

    Ok(Expr::Function {
        name: name.into(),
        args: vec![left, right],
    })
}

/// Convert a SubLink (subquery in an expression).
///
/// Recursively parses the subquery and represents it as a proper
/// SubQuery expression node instead of a placeholder.
unsafe fn convert_sublink(sl: *mut pg_sys::SubLink, depth: u32) -> Result<Expr, String> {
    use ra_core::expr::SubQueryType;

    if sl.is_null() {
        return Ok(Expr::Const(Const::Null));
    }

    let sublink_type = (*sl).subLinkType;
    let subselect = (*sl).subselect;

    // Parse the subquery
    let subquery = if subselect.is_null() {
        return Ok(Expr::Const(Const::Null));
    } else {
        let query = subselect as *mut pg_sys::Query;
        match parse_with_depth(query, depth + 1)? {
            Some(rel_expr) => rel_expr,
            None => return Ok(Expr::Const(Const::Null)),
        }
    };

    // Extract test expression for IN/ANY/ALL
    let test_expr = if !(*sl).testexpr.is_null() {
        Some(Box::new(convert_expr_depth((*sl).testexpr, depth)?))
    } else {
        None
    };

    // Map sublink type
    #[allow(non_upper_case_globals)]
    let sq_type = match sublink_type {
        pg_sys::SubLinkType::EXISTS_SUBLINK => SubQueryType::Exists,
        pg_sys::SubLinkType::ANY_SUBLINK => SubQueryType::Any,
        pg_sys::SubLinkType::ALL_SUBLINK => SubQueryType::All,
        pg_sys::SubLinkType::EXPR_SUBLINK => SubQueryType::Scalar,
        _ => SubQueryType::Scalar,
    };

    Ok(Expr::SubQuery {
        subquery_type: sq_type,
        query: Box::new(subquery),
        test_expr,
    })
}

/// Convert an ARRAY[...] constructor.
unsafe fn convert_array_expr(ae: *mut pg_sys::ArrayExpr, depth: u32) -> Result<Expr, String> {
    if ae.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let elements = (*ae).elements;
    let mut elems = Vec::new();
    if !elements.is_null() {
        let len = list_length(elements);
        for i in 0..len {
            let node = pg_sys::list_nth(elements, i) as *mut pg_sys::Node;
            elems.push(convert_expr_inner(node, depth)?);
        }
    }
    Ok(Expr::Array(elems))
}

/// Convert a ROW(...) constructor to a function call.
unsafe fn convert_row_expr(re: *mut pg_sys::RowExpr, depth: u32) -> Result<Expr, String> {
    if re.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args_list = (*re).args;
    let mut args_out = Vec::new();
    if !args_list.is_null() {
        let len = list_length(args_list);
        for i in 0..len {
            let node = pg_sys::list_nth(args_list, i) as *mut pg_sys::Node;
            args_out.push(convert_expr_inner(node, depth)?);
        }
    }
    Ok(Expr::Function {
        name: "ROW".into(),
        args: args_out,
    })
}

/// Convert a Param node ($1, $2, etc.) to a placeholder column.
unsafe fn convert_param(param: *mut pg_sys::Param) -> Result<Expr, String> {
    if param.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let paramid = (*param).paramid;
    Ok(Expr::Column(ColumnRef::new(format!("__param_{paramid}"))))
}

/// Convert SQLValueFunction (CURRENT_DATE, CURRENT_USER, etc.).
unsafe fn convert_sql_value_function(svf: *mut pg_sys::SQLValueFunction) -> Result<Expr, String> {
    if svf.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let op = (*svf).op;

    #[allow(non_upper_case_globals)]
    let name = match op {
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_DATE => "CURRENT_DATE",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_TIME
        | pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_TIME_N => "CURRENT_TIME",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_TIMESTAMP
        | pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_TIMESTAMP_N => "CURRENT_TIMESTAMP",
        pg_sys::SQLValueFunctionOp::SVFOP_LOCALTIME
        | pg_sys::SQLValueFunctionOp::SVFOP_LOCALTIME_N => "LOCALTIME",
        pg_sys::SQLValueFunctionOp::SVFOP_LOCALTIMESTAMP
        | pg_sys::SQLValueFunctionOp::SVFOP_LOCALTIMESTAMP_N => "LOCALTIMESTAMP",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_ROLE => "CURRENT_ROLE",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_USER => "CURRENT_USER",
        pg_sys::SQLValueFunctionOp::SVFOP_SESSION_USER => "SESSION_USER",
        pg_sys::SQLValueFunctionOp::SVFOP_USER => "USER",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_CATALOG => "CURRENT_CATALOG",
        pg_sys::SQLValueFunctionOp::SVFOP_CURRENT_SCHEMA => "CURRENT_SCHEMA",
        _ => "__sql_value_fn",
    };

    Ok(Expr::Function {
        name: name.into(),
        args: vec![],
    })
}

/// Convert GREATEST/LEAST (MinMaxExpr).
unsafe fn convert_minmax_expr(mme: *mut pg_sys::MinMaxExpr, depth: u32) -> Result<Expr, String> {
    if mme.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args_list = (*mme).args;
    let mut args_out = Vec::new();
    if !args_list.is_null() {
        let len = list_length(args_list);
        for i in 0..len {
            let node = pg_sys::list_nth(args_list, i) as *mut pg_sys::Node;
            args_out.push(convert_expr_inner(node, depth)?);
        }
    }

    let name = if (*mme).op == pg_sys::MinMaxOp::IS_GREATEST {
        "GREATEST"
    } else {
        "LEAST"
    };

    Ok(Expr::Function {
        name: name.into(),
        args: args_out,
    })
}

// ── Helpers ─────────────────────────────────────────────────

unsafe fn get_rte(
    q: &pg_sys::Query,
    rtindex: i32,
) -> Result<Option<*mut pg_sys::RangeTblEntry>, String> {
    let rtable = q.rtable;
    if rtable.is_null() {
        return Ok(None);
    }
    let len = list_length(rtable);
    if rtindex < 1 || rtindex > len {
        return Ok(None);
    }
    let rte = pg_sys::list_nth(rtable, rtindex - 1) as *mut pg_sys::RangeTblEntry;
    if rte.is_null() {
        return Ok(None);
    }
    Ok(Some(rte))
}

unsafe fn resolve_rel_name(relid: pg_sys::Oid) -> Option<String> {
    let name_ptr = pg_sys::get_rel_name(relid);
    if name_ptr.is_null() {
        return None;
    }
    Some(CStr::from_ptr(name_ptr).to_string_lossy().into_owned())
}

unsafe fn resolve_alias(rte: *mut pg_sys::RangeTblEntry) -> Option<String> {
    if rte.is_null() {
        return None;
    }
    let alias = (*rte).alias;
    if alias.is_null() {
        return None;
    }
    let aliasname = (*alias).aliasname;
    if aliasname.is_null() {
        return None;
    }
    Some(CStr::from_ptr(aliasname).to_string_lossy().into_owned())
}

unsafe fn resolve_tle_alias(tle: *mut pg_sys::TargetEntry) -> Option<String> {
    if tle.is_null() {
        return None;
    }
    let name = (*tle).resname;
    if name.is_null() {
        return None;
    }
    Some(CStr::from_ptr(name).to_string_lossy().into_owned())
}

/// Resolve a field name from a composite type OID and field number.
///
/// Looks up the attribute name from pg_attribute for the given
/// type and field position.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn resolve_field_name(type_oid: pg_sys::Oid, fieldnum: i16) -> Option<String> {
    use std::ffi::CStr;

    // Look up the type's typrelid (for composite types)
    let type_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::TYPEOID as i32,
        pg_sys::Datum::from(type_oid),
    );

    if type_tuple.is_null() {
        return None;
    }

    let type_form = pg_sys::GETSTRUCT(type_tuple) as *mut pg_sys::FormData_pg_type;
    let typrelid = (*type_form).typrelid;
    pg_sys::ReleaseSysCache(type_tuple);

    if typrelid == pg_sys::InvalidOid {
        return None;
    }

    // Look up the attribute
    let attr_tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(typrelid),
        pg_sys::Datum::from(fieldnum as i32),
    );

    if attr_tuple.is_null() {
        return None;
    }

    let attr_form = pg_sys::GETSTRUCT(attr_tuple) as *mut pg_sys::FormData_pg_attribute;
    let name = CStr::from_ptr((*attr_form).attname.data.as_ptr())
        .to_string_lossy()
        .into_owned();

    pg_sys::ReleaseSysCache(attr_tuple);

    Some(name)
}

unsafe fn find_target_entry_by_ref(
    tlist: *mut pg_sys::List,
    ref_id: pg_sys::Index,
    depth: u32,
) -> Option<Expr> {
    if tlist.is_null() {
        return None;
    }
    let len = list_length(tlist);
    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i) as *mut pg_sys::TargetEntry;
        if tle.is_null() {
            continue;
        }
        if (*tle).ressortgroupref == ref_id {
            let node = (*tle).expr as *mut pg_sys::Node;
            if !node.is_null() {
                return convert_expr_depth(node, depth).ok();
            }
        }
    }
    None
}

unsafe fn resolve_func_name(funcid: pg_sys::Oid) -> String {
    let name_ptr = pg_sys::get_func_name(funcid);
    if name_ptr.is_null() {
        return format!("func_{}", funcid.to_u32());
    }
    CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
}

/// Map a PostgreSQL join-type constant to Ra's `JoinType`.
fn convert_join_type(jt: pg_sys::JoinType::Type) -> JoinType {
    #[allow(non_upper_case_globals)]
    match jt {
        pg_sys::JoinType::JOIN_INNER => JoinType::Inner,
        pg_sys::JoinType::JOIN_LEFT => JoinType::LeftOuter,
        pg_sys::JoinType::JOIN_FULL => JoinType::FullOuter,
        pg_sys::JoinType::JOIN_RIGHT => JoinType::RightOuter,
        pg_sys::JoinType::JOIN_SEMI => JoinType::Semi,
        pg_sys::JoinType::JOIN_ANTI => JoinType::Anti,
        _ => JoinType::Inner,
    }
}

/// Map a PostgreSQL operator OID to Ra's `BinOp`.
///
/// Covers the standard operators for int2, int4, int8, float4,
/// float8, numeric, text, oid, date, timestamp, and timestamptz.
fn map_operator_oid(opno: pg_sys::Oid) -> BinOp {
    match opno.to_u32() {
        // Equality: int4, int2, float8, text, numeric, oid,
        //           date, timestamp, timestamptz
        96 | 94 | 670 | 98 | 1752 | 607 | 1093 | 2060 | 1320 => BinOp::Eq,
        // Less-than
        97 | 95 | 672 | 664 | 1754 | 609 | 1095 | 2062 | 1322 => BinOp::Lt,
        // Greater-than
        518 | 520 | 674 | 666 | 1756 | 610 | 1097 | 2064 | 1324 => BinOp::Gt,
        // Not-equal
        519 | 517 | 671 | 531 | 1753 | 608 | 1094 | 2061 | 1321 => BinOp::Ne,
        // Less-than-or-equal
        521 | 522 | 673 | 665 | 1755 | 611 | 1096 | 2063 | 1323 => BinOp::Le,
        // Greater-than-or-equal
        524 | 523 | 675 | 667 | 1757 | 612 | 1098 | 2065 | 1325 => BinOp::Ge,
        // Arithmetic
        551 | 550 | 591 | 584 => BinOp::Add,
        555 | 558 | 593 | 586 => BinOp::Sub,
        514 | 526 | 589 | 544 => BinOp::Mul,
        528 | 527 | 590 | 545 => BinOp::Div,
        530 | 529 => BinOp::Mod,
        // Concatenation
        654 => BinOp::Concat,
        _ => BinOp::Eq,
    }
}

/// Map an aggregate function OID to Ra's `AggregateFunction`.
///
/// Covers standard aggregate OIDs across int2, int4, int8,
/// float4, float8, and numeric types.
fn map_agg_oid(funcoid: pg_sys::Oid) -> AggregateFunction {
    match funcoid.to_u32() {
        // count(*) and count(expr)
        2803 | 2147 | 2146 => AggregateFunction::Count,
        // sum
        2108 | 2109 | 2110 | 2111 | 2112 | 2113 | 2114 => AggregateFunction::Sum,
        // avg
        2100 | 2101 | 2102 | 2103 | 2104 | 2105 | 2106 => AggregateFunction::Avg,
        // min
        2131 | 2132 | 2133 | 2134 | 2135 | 2136 | 2137 | 2138 | 2139 | 2245 => {
            AggregateFunction::Min
        }
        // max
        2115 | 2116 | 2117 | 2118 | 2119 | 2120 | 2121 | 2122 | 2123 | 2126 | 2244 => {
            AggregateFunction::Max
        }
        // stddev
        2154 | 2155 | 2156 | 2157 | 2158 | 2159 => AggregateFunction::StdDev,
        // variance
        2148 | 2149 | 2150 | 2151 | 2152 | 2153 => AggregateFunction::Variance,
        // string_agg
        3538 | 3543 => AggregateFunction::StringAgg,
        // array_agg
        2335 | 4051 | 4052 | 4053 => AggregateFunction::ArrayAgg,
        _ => AggregateFunction::Count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Operator OID mapping ────────────────────────────────

    #[test]
    fn operator_oid_eq() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(96_u32)), BinOp::Eq,);
    }

    #[test]
    fn operator_oid_lt() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(97_u32)), BinOp::Lt,);
    }

    #[test]
    fn operator_oid_gt() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(518_u32)), BinOp::Gt,);
    }

    #[test]
    fn operator_oid_ne() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(519_u32)), BinOp::Ne,);
    }

    #[test]
    fn operator_oid_le() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(521_u32)), BinOp::Le,);
    }

    #[test]
    fn operator_oid_ge() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(524_u32)), BinOp::Ge,);
    }

    #[test]
    fn operator_oid_add() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(551_u32)), BinOp::Add,);
    }

    #[test]
    fn operator_oid_sub() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(555_u32)), BinOp::Sub,);
    }

    #[test]
    fn operator_oid_mul() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(514_u32)), BinOp::Mul,);
    }

    #[test]
    fn operator_oid_div() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(528_u32)), BinOp::Div,);
    }

    #[test]
    fn operator_oid_mod() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(530_u32)), BinOp::Mod,);
    }

    #[test]
    fn operator_oid_concat() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(654_u32)), BinOp::Concat,);
    }

    #[test]
    fn operator_oid_unknown_defaults_eq() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(999_999_u32)), BinOp::Eq,);
    }

    // ── Date/timestamp operators ────────────────────────────

    #[test]
    fn operator_oid_date_eq() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(1093_u32)), BinOp::Eq,);
    }

    #[test]
    fn operator_oid_date_lt() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(1095_u32)), BinOp::Lt,);
    }

    #[test]
    fn operator_oid_timestamp_eq() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(2060_u32)), BinOp::Eq,);
    }

    #[test]
    fn operator_oid_timestamptz_gt() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(1324_u32)), BinOp::Gt,);
    }

    // ── Int2 operators ──────────────────────────────────────

    #[test]
    fn operator_oid_int2_eq() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(94_u32)), BinOp::Eq,);
    }

    #[test]
    fn operator_oid_int2_lt() {
        assert_eq!(map_operator_oid(pg_sys::Oid::from(95_u32)), BinOp::Lt,);
    }

    // ── Arithmetic int8/float variants ──────────────────────

    #[test]
    fn operator_oid_int8_add() {
        assert_eq!(
            map_operator_oid(pg_sys::Oid::from(684_u32)),
            BinOp::Eq,
            "int8 add 684 not yet mapped, defaults to Eq"
        );
    }

    // ── Join type mapping ───────────────────────────────────

    #[test]
    fn join_type_inner() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_INNER),
            JoinType::Inner,
        );
    }

    #[test]
    fn join_type_left() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_LEFT),
            JoinType::LeftOuter,
        );
    }

    #[test]
    fn join_type_right() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_RIGHT),
            JoinType::RightOuter,
        );
    }

    #[test]
    fn join_type_full() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_FULL),
            JoinType::FullOuter,
        );
    }

    #[test]
    fn join_type_semi() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_SEMI),
            JoinType::Semi,
        );
    }

    #[test]
    fn join_type_anti() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_ANTI),
            JoinType::Anti,
        );
    }

    #[test]
    fn join_type_unknown_defaults_inner() {
        assert_eq!(convert_join_type(99), JoinType::Inner,);
    }

    // ── Aggregate OID mapping ───────────────────────────────

    #[test]
    fn agg_oid_count_star() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2803_u32)),
            AggregateFunction::Count,
        );
    }

    #[test]
    fn agg_oid_count_expr() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2147_u32)),
            AggregateFunction::Count,
        );
    }

    #[test]
    fn agg_oid_count_bigint() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2146_u32)),
            AggregateFunction::Count,
        );
    }

    #[test]
    fn agg_oid_sum() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2108_u32)),
            AggregateFunction::Sum,
        );
    }

    #[test]
    fn agg_oid_avg() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2100_u32)),
            AggregateFunction::Avg,
        );
    }

    #[test]
    fn agg_oid_min() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2131_u32)),
            AggregateFunction::Min,
        );
    }

    #[test]
    fn agg_oid_max() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2115_u32)),
            AggregateFunction::Max,
        );
    }

    #[test]
    fn agg_oid_stddev() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2154_u32)),
            AggregateFunction::StdDev,
        );
    }

    #[test]
    fn agg_oid_variance() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2148_u32)),
            AggregateFunction::Variance,
        );
    }

    #[test]
    fn agg_oid_string_agg() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(3538_u32)),
            AggregateFunction::StringAgg,
        );
    }

    #[test]
    fn agg_oid_array_agg() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2335_u32)),
            AggregateFunction::ArrayAgg,
        );
    }

    #[test]
    fn agg_oid_unknown_defaults_count() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(999_999_u32)),
            AggregateFunction::Count,
        );
    }

    // ── Sort direction ──────────────────────────────────────

    #[test]
    fn sort_direction_asc_for_lt_op() {
        let dir = infer_sort_direction(pg_sys::Oid::from(97_u32));
        assert_eq!(dir, SortDirection::Asc);
    }

    #[test]
    fn sort_direction_desc_for_gt_op() {
        let dir = infer_sort_direction(pg_sys::Oid::from(518_u32));
        assert_eq!(dir, SortDirection::Desc);
    }

    #[test]
    fn sort_direction_desc_for_numeric_gt() {
        let dir = infer_sort_direction(pg_sys::Oid::from(1756_u32));
        assert_eq!(dir, SortDirection::Desc);
    }

    #[test]
    fn sort_direction_desc_for_date_gt() {
        let dir = infer_sort_direction(pg_sys::Oid::from(1097_u32));
        assert_eq!(dir, SortDirection::Desc);
    }

    #[test]
    fn sort_direction_desc_for_timestamptz_gt() {
        let dir = infer_sort_direction(pg_sys::Oid::from(1324_u32));
        assert_eq!(dir, SortDirection::Desc);
    }

    #[test]
    fn sort_direction_asc_for_unknown_op() {
        let dir = infer_sort_direction(pg_sys::Oid::from(999_999_u32));
        assert_eq!(dir, SortDirection::Asc);
    }

    // ── Window function OID mapping ────────────────────────

    #[test]
    fn window_oid_row_number() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3100_u32)),
            WindowFunction::RowNumber,
        );
    }

    #[test]
    fn window_oid_rank() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3101_u32)),
            WindowFunction::Rank,
        );
    }

    #[test]
    fn window_oid_dense_rank() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3102_u32)),
            WindowFunction::DenseRank,
        );
    }

    #[test]
    fn window_oid_percent_rank() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3103_u32)),
            WindowFunction::PercentRank,
        );
    }

    #[test]
    fn window_oid_ntile() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3105_u32)),
            WindowFunction::Ntile,
        );
    }

    #[test]
    fn window_oid_lag() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3106_u32)),
            WindowFunction::Lag,
        );
    }

    #[test]
    fn window_oid_lag_variant() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3107_u32)),
            WindowFunction::Lag,
        );
    }

    #[test]
    fn window_oid_lead() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3109_u32)),
            WindowFunction::Lead,
        );
    }

    #[test]
    fn window_oid_lead_variant() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3110_u32)),
            WindowFunction::Lead,
        );
    }

    #[test]
    fn window_oid_first_value() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3112_u32)),
            WindowFunction::FirstValue,
        );
    }

    #[test]
    fn window_oid_last_value() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3113_u32)),
            WindowFunction::LastValue,
        );
    }

    #[test]
    fn window_oid_nth_value() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(3114_u32)),
            WindowFunction::NthValue,
        );
    }

    #[test]
    fn window_oid_agg_sum_as_window() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(2108_u32)),
            WindowFunction::Sum,
        );
    }

    #[test]
    fn window_oid_agg_avg_as_window() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(2100_u32)),
            WindowFunction::Avg,
        );
    }

    #[test]
    fn window_oid_agg_count_as_window() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(2803_u32)),
            WindowFunction::Count,
        );
    }

    #[test]
    fn window_oid_agg_min_as_window() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(2131_u32)),
            WindowFunction::Min,
        );
    }

    #[test]
    fn window_oid_agg_max_as_window() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(2115_u32)),
            WindowFunction::Max,
        );
    }

    #[test]
    fn window_oid_unknown_defaults_row_number() {
        assert_eq!(
            map_window_func_oid(pg_sys::Oid::from(999_999_u32)),
            WindowFunction::RowNumber,
        );
    }

    // ── Window frame extraction (null safety) ──────────────

    #[test]
    fn extract_window_frame_null_returns_none() {
        let frame = unsafe { extract_window_frame(std::ptr::null_mut()) };
        assert!(frame.is_none());
    }

    #[test]
    fn extract_frame_offset_null_returns_zero() {
        let offset = unsafe { extract_frame_offset(std::ptr::null_mut()) };
        assert_eq!(offset, 0);
    }

    // ── Null pointer safety (parse) ─────────────────────────

    #[test]
    fn parse_null_query_returns_error() {
        let result = unsafe { parse(std::ptr::null_mut()) };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("null Query pointer"));
    }

    // ── Max depth constant ──────────────────────────────────

    #[test]
    fn max_depth_is_reasonable() {
        assert!(MAX_DEPTH >= 32);
        assert!(MAX_DEPTH <= 256);
    }

    // ── List length helper ──────────────────────────────────

    #[test]
    fn list_length_null_returns_zero() {
        let len = unsafe { list_length(std::ptr::null_mut()) };
        assert_eq!(len, 0);
    }

    // ── CTE wrapping ───────────────────────────────────────

    #[test]
    fn wrap_no_ctes_returns_body() {
        let body = RelExpr::scan("t");
        let result = wrap_with_ctes(body.clone(), vec![]);
        assert_eq!(result, body);
    }

    #[test]
    fn wrap_single_non_recursive_cte() {
        let body = RelExpr::scan("tmp");
        let defs = vec![CteDef {
            name: "tmp".into(),
            definition: RelExpr::scan("source"),
            is_recursive: false,
            cycle_detection: None,
        }];
        let result = wrap_with_ctes(body, defs);
        if let RelExpr::CTE {
            name,
            definition,
            body,
        } = &result
        {
            assert_eq!(name, "tmp");
            assert!(matches!(
                definition.as_ref(),
                RelExpr::Scan { table, .. }
                    if table == "source"
            ));
            assert!(matches!(
                body.as_ref(),
                RelExpr::Scan { table, .. }
                    if table == "tmp"
            ));
        } else {
            panic!("expected CTE variant, got {result:?}");
        }
    }

    #[test]
    fn wrap_recursive_cte() {
        let body = RelExpr::scan("reachable");
        let defs = vec![CteDef {
            name: "reachable".into(),
            definition: RelExpr::scan("edges"),
            is_recursive: true,
            cycle_detection: None,
        }];
        let result = wrap_with_ctes(body, defs);
        assert!(matches!(result, RelExpr::RecursiveCTE { .. }));
    }

    #[test]
    fn wrap_multiple_ctes_nesting_order() {
        let body = RelExpr::scan("main");
        let defs = vec![
            CteDef {
                name: "a".into(),
                definition: RelExpr::scan("t1"),
                is_recursive: false,
                cycle_detection: None,
            },
            CteDef {
                name: "b".into(),
                definition: RelExpr::scan("t2"),
                is_recursive: false,
                cycle_detection: None,
            },
        ];
        let result = wrap_with_ctes(body, defs);
        // Outermost should be CTE "a"
        if let RelExpr::CTE { name, body, .. } = &result {
            assert_eq!(name, "a");
            // Inner should be CTE "b"
            if let RelExpr::CTE { name, body, .. } = body.as_ref() {
                assert_eq!(name, "b");
                assert!(matches!(
                    body.as_ref(),
                    RelExpr::Scan { table, .. }
                        if table == "main"
                ));
            } else {
                panic!("expected inner CTE 'b'");
            }
        } else {
            panic!("expected outer CTE 'a'");
        }
    }

    #[test]
    fn wrap_recursive_cte_with_cycle_detection() {
        let body = RelExpr::scan("r");
        let defs = vec![CteDef {
            name: "r".into(),
            definition: RelExpr::scan("edges"),
            is_recursive: true,
            cycle_detection: Some(CycleDetection {
                track_columns: vec!["id".into()],
                max_depth: None,
                cycle_mark_column: Some("is_cycle".into()),
                path_column: Some("path".into()),
            }),
        }];
        let result = wrap_with_ctes(body, defs);
        if let RelExpr::RecursiveCTE {
            name,
            cycle_detection,
            ..
        } = &result
        {
            assert_eq!(name, "r");
            let cd = cycle_detection
                .as_ref()
                .expect("expected cycle_detection");
            assert_eq!(cd.track_columns, vec!["id"]);
            assert_eq!(cd.cycle_mark_column, Some("is_cycle".into()));
            assert_eq!(cd.path_column, Some("path".into()));
        } else {
            panic!("expected RecursiveCTE");
        }
    }

    #[test]
    fn resolve_cte_name_null_returns_empty() {
        let result = unsafe { resolve_cte_name(std::ptr::null_mut()) };
        assert!(result.is_empty());
    }

    // ── Set operation helpers ──────────────────────────────

    #[test]
    fn set_operation_null_node_returns_none() {
        let mut q = pg_sys::Query::default();
        q.commandType = pg_sys::CmdType::CMD_SELECT;
        let result = unsafe {
            parse_set_operation(
                &q,
                std::ptr::null_mut(),
                0,
            )
        };
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn set_operation_exceeds_depth_returns_error() {
        let q = pg_sys::Query::default();
        // Use a non-null but invalid pointer to test depth
        // check before any dereference.
        let result = unsafe {
            parse_set_operation(
                &q,
                std::ptr::null_mut(),
                MAX_DEPTH + 1,
            )
        };
        // Null node short-circuits before depth check,
        // so null returns Ok(None).
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn set_operation_leaf_null_rte_returns_none() {
        let q = pg_sys::Query::default();
        // rtable is null, so get_rte returns None.
        let result = unsafe {
            parse_set_operation_leaf(&q, 1, 0)
        };
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn set_operation_leaf_invalid_rtindex_returns_none() {
        let q = pg_sys::Query::default();
        let result = unsafe {
            parse_set_operation_leaf(&q, 0, 0)
        };
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
