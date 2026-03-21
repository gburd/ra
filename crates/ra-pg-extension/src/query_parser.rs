//! Convert PostgreSQL `Query` structs into Ra `RelExpr` trees.
//!
//! Walks the PostgreSQL parse tree (rtable, jointree, targetList,
//! etc.) and produces the equivalent relational algebra
//! representation.  Returns `Ok(None)` for unsupported query types
//! so the planner hook falls back to the standard planner.

use std::ffi::CStr;

use pgrx::pg_sys;

use ra_core::algebra::{
    AggregateExpr, AggregateFunction, JoinType, NullOrdering,
    ProjectionColumn, RelExpr, SortDirection, SortKey,
};
use ra_core::expr::{BinOp, ColumnRef, Const, Expr, UnaryOp};

/// Parse a PostgreSQL `Query` into a Ra `RelExpr`.
///
/// Returns `Ok(None)` for queries we cannot represent (DDL,
/// utility statements, CTEs, window functions).
///
/// # Safety
///
/// Caller must pass a valid, non-null `Query` pointer that
/// remains live for the duration of this call.
pub unsafe fn parse(
    query: *mut pg_sys::Query,
) -> Result<Option<RelExpr>, String> {
    if query.is_null() {
        return Err("null Query pointer".into());
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

    // Bail on CTEs for now.
    if !q.cteList.is_null() && (*q.cteList).length > 0 {
        return Ok(None);
    }

    // Bail on window functions.
    if !q.windowClause.is_null()
        && (*q.windowClause).length > 0
    {
        return Ok(None);
    }

    let from_expr = build_from_clause(q)?;
    let from_expr = match from_expr {
        Some(e) => e,
        None => return Ok(None),
    };

    let filtered = apply_where_clause(q, from_expr)?;
    let aggregated = apply_group_by(q, filtered)?;
    let having_filtered = apply_having(q, aggregated)?;
    let projected = apply_projection(q, having_filtered)?;
    let distinct_applied = apply_distinct(q, projected);
    let sorted = apply_order_by(q, distinct_applied)?;
    let limited = apply_limit(q, sorted);

    Ok(Some(limited))
}

// ── FROM clause ─────────────────────────────────────────────

unsafe fn build_from_clause(
    q: &pg_sys::Query,
) -> Result<Option<RelExpr>, String> {
    let jointree = q.jointree;
    if jointree.is_null() {
        return Ok(None);
    }
    let fromlist = (*jointree).fromlist;
    if fromlist.is_null() || (*fromlist).length == 0 {
        return Ok(None);
    }

    let length = (*fromlist).length;
    let mut result: Option<RelExpr> = None;

    for i in 0..length {
        let node = pg_sys::list_nth(fromlist, i)
            as *mut pg_sys::Node;
        if node.is_null() {
            continue;
        }
        let expr = build_from_node(node, q)?;
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
) -> Result<Option<RelExpr>, String> {
    if node.is_null() {
        return Ok(None);
    }

    let tag = (*node).type_;

    if tag == pg_sys::NodeTag::T_RangeTblRef {
        let rtref = node as *mut pg_sys::RangeTblRef;
        let rtindex = (*rtref).rtindex;
        return build_rte_scan(q, rtindex);
    }

    if tag == pg_sys::NodeTag::T_JoinExpr {
        let je = node as *mut pg_sys::JoinExpr;
        return build_join_expr(je, q);
    }

    Ok(None)
}

unsafe fn build_rte_scan(
    q: &pg_sys::Query,
    rtindex: i32,
) -> Result<Option<RelExpr>, String> {
    let rte = get_rte(q, rtindex)?;
    let rte = match rte {
        Some(r) => r,
        None => return Ok(None),
    };

    if (*rte).rtekind == pg_sys::RTEKind::RTE_RELATION {
        let relid = (*rte).relid;
        let table = resolve_rel_name(relid).ok_or_else(|| {
            format!("cannot resolve OID {}", relid.to_u32())
        })?;
        let alias = resolve_alias(rte);
        return Ok(Some(RelExpr::Scan { table, alias }));
    }

    if (*rte).rtekind == pg_sys::RTEKind::RTE_SUBQUERY {
        let subquery = (*rte).subquery;
        if subquery.is_null() {
            return Ok(None);
        }
        return parse(subquery);
    }

    Ok(None)
}

unsafe fn build_join_expr(
    je: *mut pg_sys::JoinExpr,
    q: &pg_sys::Query,
) -> Result<Option<RelExpr>, String> {
    if je.is_null() {
        return Ok(None);
    }

    let left =
        build_from_node((*je).larg as *mut pg_sys::Node, q)?;
    let right =
        build_from_node((*je).rarg as *mut pg_sys::Node, q)?;

    let (left, right) = match (left, right) {
        (Some(l), Some(r)) => (l, r),
        _ => return Ok(None),
    };

    let join_type = convert_join_type((*je).jointype);

    let condition = if (*je).quals.is_null() {
        Expr::Const(Const::Bool(true))
    } else {
        convert_expr((*je).quals)?
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
) -> Result<RelExpr, String> {
    let jointree = q.jointree;
    if jointree.is_null() {
        return Ok(input);
    }
    let quals = (*jointree).quals;
    if quals.is_null() {
        return Ok(input);
    }
    let predicate = convert_expr(quals)?;
    Ok(RelExpr::Filter {
        predicate,
        input: Box::new(input),
    })
}

// ── GROUP BY + aggregates ───────────────────────────────────

unsafe fn apply_group_by(
    q: &pg_sys::Query,
    input: RelExpr,
) -> Result<RelExpr, String> {
    let has_group_by = !q.groupClause.is_null()
        && (*q.groupClause).length > 0;
    let has_aggs = q.hasAggs;

    if !has_group_by && !has_aggs {
        return Ok(input);
    }

    let group_by = extract_group_by_exprs(q)?;
    let aggregates = extract_aggregates(q)?;

    Ok(RelExpr::Aggregate {
        group_by,
        aggregates,
        input: Box::new(input),
    })
}

unsafe fn extract_group_by_exprs(
    q: &pg_sys::Query,
) -> Result<Vec<Expr>, String> {
    let mut exprs = Vec::new();
    let gc = q.groupClause;
    if gc.is_null() {
        return Ok(exprs);
    }
    let tlist = q.targetList;

    let len = (*gc).length;
    for i in 0..len {
        let sgc = pg_sys::list_nth(gc, i)
            as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        if let Some(expr) =
            find_target_entry_by_ref(tlist, tle_ref)
        {
            exprs.push(expr);
        }
    }
    Ok(exprs)
}

unsafe fn extract_aggregates(
    q: &pg_sys::Query,
) -> Result<Vec<AggregateExpr>, String> {
    let mut aggs = Vec::new();
    let tlist = q.targetList;
    if tlist.is_null() {
        return Ok(aggs);
    }

    let len = (*tlist).length;
    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i)
            as *mut pg_sys::TargetEntry;
        if tle.is_null() || (*tle).expr.is_null() {
            continue;
        }
        let node = (*tle).expr as *mut pg_sys::Node;
        if (*node).type_ == pg_sys::NodeTag::T_Aggref {
            let aggref = node as *mut pg_sys::Aggref;
            if let Some(agg) = convert_aggref(aggref, tle)? {
                aggs.push(agg);
            }
        }
    }
    Ok(aggs)
}

unsafe fn convert_aggref(
    aggref: *mut pg_sys::Aggref,
    tle: *mut pg_sys::TargetEntry,
) -> Result<Option<AggregateExpr>, String> {
    if aggref.is_null() {
        return Ok(None);
    }

    let func_oid = (*aggref).aggfnoid;
    let function = map_agg_oid(func_oid);
    let arg = extract_first_agg_arg(aggref)?;

    let distinct = {
        let d = (*aggref).aggdistinct;
        !d.is_null() && (*d).length > 0
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
) -> Result<Option<Expr>, String> {
    let args = (*aggref).args;
    if args.is_null() || (*args).length == 0 {
        return Ok(None);
    }
    let first_tle = pg_sys::list_nth(args, 0)
        as *mut pg_sys::TargetEntry;
    if first_tle.is_null() || (*first_tle).expr.is_null() {
        return Ok(None);
    }
    let expr =
        convert_expr((*first_tle).expr as *mut pg_sys::Node)?;
    Ok(Some(expr))
}

// ── HAVING ──────────────────────────────────────────────────

unsafe fn apply_having(
    q: &pg_sys::Query,
    input: RelExpr,
) -> Result<RelExpr, String> {
    let having = q.havingQual;
    if having.is_null() {
        return Ok(input);
    }
    let predicate =
        convert_expr(having as *mut pg_sys::Node)?;
    Ok(RelExpr::Filter {
        predicate,
        input: Box::new(input),
    })
}

// ── Projection (targetList) ─────────────────────────────────

unsafe fn apply_projection(
    q: &pg_sys::Query,
    input: RelExpr,
) -> Result<RelExpr, String> {
    let tlist = q.targetList;
    if tlist.is_null() || (*tlist).length == 0 {
        return Ok(input);
    }

    let mut columns = Vec::new();
    let len = (*tlist).length;

    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i)
            as *mut pg_sys::TargetEntry;
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

        let expr = convert_expr(node)?;
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

unsafe fn apply_distinct(
    q: &pg_sys::Query,
    input: RelExpr,
) -> RelExpr {
    let dc = q.distinctClause;
    if dc.is_null() || (*dc).length == 0 {
        return input;
    }
    RelExpr::Distinct {
        input: Box::new(input),
    }
}

// ── ORDER BY ────────────────────────────────────────────────

unsafe fn apply_order_by(
    q: &pg_sys::Query,
    input: RelExpr,
) -> Result<RelExpr, String> {
    let sc = q.sortClause;
    if sc.is_null() || (*sc).length == 0 {
        return Ok(input);
    }

    let tlist = q.targetList;
    let mut keys = Vec::new();
    let len = (*sc).length;

    for i in 0..len {
        let sgc = pg_sys::list_nth(sc, i)
            as *mut pg_sys::SortGroupClause;
        if sgc.is_null() {
            continue;
        }
        let tle_ref = (*sgc).tleSortGroupRef;
        let expr = find_target_entry_by_ref(tlist, tle_ref)
            .unwrap_or(Expr::Const(Const::Null));

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
    let desc_ops: &[u32] = &[
        518,  // int4gt
        413,  // int8gt
        674,  // float8gt
        666,  // textgt
        1756, // numericgt
    ];

    if desc_ops.contains(&sortop.to_u32()) {
        SortDirection::Desc
    } else {
        SortDirection::Asc
    }
}

// ── LIMIT / OFFSET ──────────────────────────────────────────

unsafe fn apply_limit(
    q: &pg_sys::Query,
    input: RelExpr,
) -> RelExpr {
    let limit_node = q.limitCount;
    let offset_node = q.limitOffset;

    if limit_node.is_null() && offset_node.is_null() {
        return input;
    }

    let count =
        extract_const_u64(limit_node).unwrap_or(u64::MAX);
    let offset =
        extract_const_u64(offset_node).unwrap_or(0);

    RelExpr::Limit {
        count,
        offset,
        input: Box::new(input),
    }
}

unsafe fn extract_const_u64(
    node: *mut pg_sys::Node,
) -> Option<u64> {
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
    #[allow(clippy::cast_sign_loss)]
    let val = (*con).constvalue.value() as i64;
    if val < 0 {
        None
    } else {
        Some(val as u64)
    }
}

// ── Expression conversion ───────────────────────────────────

unsafe fn convert_expr(
    node: *mut pg_sys::Node,
) -> Result<Expr, String> {
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
        return convert_opexpr(node as *mut pg_sys::OpExpr);
    }
    if tag == pg_sys::NodeTag::T_BoolExpr {
        return convert_boolexpr(
            node as *mut pg_sys::BoolExpr,
        );
    }
    if tag == pg_sys::NodeTag::T_NullTest {
        return convert_nulltest(
            node as *mut pg_sys::NullTest,
        );
    }
    if tag == pg_sys::NodeTag::T_FuncExpr {
        return convert_funcexpr(
            node as *mut pg_sys::FuncExpr,
        );
    }
    if tag == pg_sys::NodeTag::T_Aggref {
        return convert_aggref_as_expr(
            node as *mut pg_sys::Aggref,
        );
    }
    if tag == pg_sys::NodeTag::T_RelabelType {
        let rt = node as *mut pg_sys::RelabelType;
        return convert_expr((*rt).arg as *mut pg_sys::Node);
    }
    if tag == pg_sys::NodeTag::T_CoerceViaIO {
        let cio = node as *mut pg_sys::CoerceViaIO;
        return convert_expr((*cio).arg as *mut pg_sys::Node);
    }

    Ok(Expr::Column(ColumnRef::new(format!(
        "__pg_node_{tag:?}"
    ))))
}

unsafe fn convert_var(
    var: *mut pg_sys::Var,
) -> Result<Expr, String> {
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

unsafe fn convert_pg_const(
    con: *mut pg_sys::Const,
) -> Result<Expr, String> {
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
        // FLOAT4OID
        700 => {
            #[allow(clippy::cast_possible_truncation)]
            let bits = datum_val as u32;
            Const::Float(f64::from(f32::from_bits(bits)))
        }
        // FLOAT8OID
        701 => {
            let bits = datum_val as u64;
            Const::Float(f64::from_bits(bits))
        }
        // TEXTOID, VARCHAROID, NAMEOID
        25 | 1043 | 19 => {
            if datum_val == 0 {
                Const::Null
            } else {
                let vl = datum_val as *const u8;
                let text_ptr = vl.add(4) as *const i8;
                let s = CStr::from_ptr(text_ptr)
                    .to_string_lossy()
                    .into_owned();
                Const::String(s)
            }
        }
        // NUMERICOID
        1700 => Const::Float(0.0),
        _ => Const::Int(datum_val as i64),
    };

    Ok(Expr::Const(val))
}

unsafe fn convert_opexpr(
    opexpr: *mut pg_sys::OpExpr,
) -> Result<Expr, String> {
    if opexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let args = (*opexpr).args;
    if args.is_null() || (*args).length < 2 {
        return Ok(Expr::Const(Const::Null));
    }

    let left_node =
        pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    let right_node =
        pg_sys::list_nth(args, 1) as *mut pg_sys::Node;

    let left = convert_expr(left_node)?;
    let right = convert_expr(right_node)?;
    let op = map_operator_oid((*opexpr).opno);

    Ok(Expr::BinOp {
        op,
        left: Box::new(left),
        right: Box::new(right),
    })
}

unsafe fn convert_boolexpr(
    bexpr: *mut pg_sys::BoolExpr,
) -> Result<Expr, String> {
    if bexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }

    let boolop = (*bexpr).boolop;
    let args = (*bexpr).args;
    if args.is_null() || (*args).length == 0 {
        return Ok(Expr::Const(Const::Bool(true)));
    }

    if boolop == pg_sys::BoolExprType::NOT_EXPR {
        let child =
            pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
        let operand = convert_expr(child)?;
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

    let len = (*args).length;
    let first =
        pg_sys::list_nth(args, 0) as *mut pg_sys::Node;
    let mut acc = convert_expr(first)?;

    for i in 1..len {
        let next =
            pg_sys::list_nth(args, i) as *mut pg_sys::Node;
        let rhs = convert_expr(next)?;
        acc = Expr::BinOp {
            op: binop,
            left: Box::new(acc),
            right: Box::new(rhs),
        };
    }

    Ok(acc)
}

unsafe fn convert_nulltest(
    nt: *mut pg_sys::NullTest,
) -> Result<Expr, String> {
    if nt.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let arg = convert_expr((*nt).arg as *mut pg_sys::Node)?;
    let op = if (*nt).nulltesttype
        == pg_sys::NullTestType::IS_NULL
    {
        UnaryOp::IsNull
    } else {
        UnaryOp::IsNotNull
    };
    Ok(Expr::UnaryOp {
        op,
        operand: Box::new(arg),
    })
}

unsafe fn convert_funcexpr(
    funcexpr: *mut pg_sys::FuncExpr,
) -> Result<Expr, String> {
    if funcexpr.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let funcid = (*funcexpr).funcid;
    let name = resolve_func_name(funcid);

    let mut args_out = Vec::new();
    let args = (*funcexpr).args;
    if !args.is_null() {
        let len = (*args).length;
        for i in 0..len {
            let node = pg_sys::list_nth(args, i)
                as *mut pg_sys::Node;
            args_out.push(convert_expr(node)?);
        }
    }

    Ok(Expr::Function {
        name,
        args: args_out,
    })
}

unsafe fn convert_aggref_as_expr(
    aggref: *mut pg_sys::Aggref,
) -> Result<Expr, String> {
    if aggref.is_null() {
        return Ok(Expr::Const(Const::Null));
    }
    let func = map_agg_oid((*aggref).aggfnoid);
    let name = format!("{func}");

    let mut args_out = Vec::new();
    let args = (*aggref).args;
    if !args.is_null() {
        let len = (*args).length;
        for i in 0..len {
            let tle = pg_sys::list_nth(args, i)
                as *mut pg_sys::TargetEntry;
            if !tle.is_null() && !(*tle).expr.is_null() {
                args_out.push(convert_expr(
                    (*tle).expr as *mut pg_sys::Node,
                )?);
            }
        }
    }

    Ok(Expr::Function {
        name,
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
    if rtindex < 1 || rtindex > (*rtable).length {
        return Ok(None);
    }
    let rte = pg_sys::list_nth(rtable, rtindex - 1)
        as *mut pg_sys::RangeTblEntry;
    Ok(Some(rte))
}

unsafe fn resolve_rel_name(
    relid: pg_sys::Oid,
) -> Option<String> {
    let name_ptr = pg_sys::get_rel_name(relid);
    if name_ptr.is_null() {
        return None;
    }
    Some(
        CStr::from_ptr(name_ptr)
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn resolve_alias(
    rte: *mut pg_sys::RangeTblEntry,
) -> Option<String> {
    let alias = (*rte).alias;
    if alias.is_null() {
        return None;
    }
    let aliasname = (*alias).aliasname;
    if aliasname.is_null() {
        return None;
    }
    Some(
        CStr::from_ptr(aliasname)
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn resolve_tle_alias(
    tle: *mut pg_sys::TargetEntry,
) -> Option<String> {
    if tle.is_null() {
        return None;
    }
    let name = (*tle).resname;
    if name.is_null() {
        return None;
    }
    Some(CStr::from_ptr(name).to_string_lossy().into_owned())
}

unsafe fn find_target_entry_by_ref(
    tlist: *mut pg_sys::List,
    ref_id: pg_sys::Index,
) -> Option<Expr> {
    if tlist.is_null() {
        return None;
    }
    let len = (*tlist).length;
    for i in 0..len {
        let tle = pg_sys::list_nth(tlist, i)
            as *mut pg_sys::TargetEntry;
        if tle.is_null() {
            continue;
        }
        if (*tle).ressortgroupref == ref_id {
            let node = (*tle).expr as *mut pg_sys::Node;
            if !node.is_null() {
                return convert_expr(node).ok();
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
fn map_operator_oid(opno: pg_sys::Oid) -> BinOp {
    match opno.to_u32() {
        96 | 410 | 670 | 98 | 1752 => BinOp::Eq,
        97 | 412 | 672 | 664 | 1754 => BinOp::Lt,
        518 | 413 | 674 | 666 | 1756 => BinOp::Gt,
        520 | 411 | 671 | 531 | 1753 => BinOp::Ne,
        521 | 414 | 673 | 665 | 1755 => BinOp::Le,
        524 | 415 | 675 | 667 | 1757 => BinOp::Ge,
        551 => BinOp::Add,
        555 => BinOp::Sub,
        514 => BinOp::Mul,
        528 => BinOp::Div,
        654 => BinOp::Concat,
        _ => BinOp::Eq,
    }
}

/// Map an aggregate function OID to Ra's `AggregateFunction`.
fn map_agg_oid(funcoid: pg_sys::Oid) -> AggregateFunction {
    match funcoid.to_u32() {
        2803 | 2147 => AggregateFunction::Count,
        2108 | 2109 | 2110 | 2111 => AggregateFunction::Sum,
        2100 | 2101 | 2102 | 2103 | 2104 | 2105 | 2106 => {
            AggregateFunction::Avg
        }
        2131 | 2132 | 2133 | 2134 | 2135 | 2136 | 2137
        | 2138 | 2139 => AggregateFunction::Min,
        2115 | 2116 | 2117 | 2118 | 2119 | 2120 | 2121
        | 2122 | 2123 | 2126 => AggregateFunction::Max,
        2154 | 2155 | 2156 | 2157 => {
            AggregateFunction::StdDev
        }
        2148 | 2149 | 2150 | 2151 => {
            AggregateFunction::Variance
        }
        3538 => AggregateFunction::StringAgg,
        2335 => AggregateFunction::ArrayAgg,
        _ => AggregateFunction::Count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_oid_eq() {
        assert_eq!(
            map_operator_oid(pg_sys::Oid::from(96_u32)),
            BinOp::Eq,
        );
    }

    #[test]
    fn operator_oid_lt() {
        assert_eq!(
            map_operator_oid(pg_sys::Oid::from(97_u32)),
            BinOp::Lt,
        );
    }

    #[test]
    fn operator_oid_unknown_defaults_eq() {
        assert_eq!(
            map_operator_oid(pg_sys::Oid::from(999_999_u32)),
            BinOp::Eq,
        );
    }

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
    fn join_type_full() {
        assert_eq!(
            convert_join_type(pg_sys::JoinType::JOIN_FULL),
            JoinType::FullOuter,
        );
    }

    #[test]
    fn agg_oid_count_star() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(2803_u32)),
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
    fn agg_oid_unknown_defaults_count() {
        assert_eq!(
            map_agg_oid(pg_sys::Oid::from(999_999_u32)),
            AggregateFunction::Count,
        );
    }

    #[test]
    fn sort_direction_asc_for_lt_op() {
        let dir =
            infer_sort_direction(pg_sys::Oid::from(97_u32));
        assert_eq!(dir, SortDirection::Asc);
    }

    #[test]
    fn sort_direction_desc_for_gt_op() {
        let dir =
            infer_sort_direction(pg_sys::Oid::from(518_u32));
        assert_eq!(dir, SortDirection::Desc);
    }
}
