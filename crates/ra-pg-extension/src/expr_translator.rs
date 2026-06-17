//! Expression translation: Ra `Expr` → PostgreSQL `Node*`.
//!
//! Translates Ra's expression tree into PostgreSQL executor-compatible
//! expression nodes. Each Ra expression variant maps to the corresponding
//! PostgreSQL node type.
//!
//! # Coverage
//!
//! | Ra `Expr` variant              | PostgreSQL node         |
//! |--------------------------------|------------------------|
//! | `Const::Null`                  | `Const` (null)         |
//! | `Const::Bool / Int / Float`    | `Const` (typed)        |
//! | `Const::String`                | `Const` (text)         |
//! | `Column(ref)`                  | `Var`                  |
//! | `BinOp::And / Or`              | `BoolExpr`             |
//! | `BinOp::Eq / Ne / Lt / …`     | `OpExpr`               |
//! | `BinOp::Add / Sub / Mul / Div` | `OpExpr`               |
//! | `UnaryOp::Not`                 | `BoolExpr (NOT)`       |
//! | `UnaryOp::IsNull / IsNotNull`  | `NullTest`             |
//! | `UnaryOp::Neg`                 | `OpExpr` (unary -)     |
//! | `Function(name, args)`         | `FuncExpr`             |
//! | `Case`                         | `CaseExpr`             |
//! | `Cast`                         | `CoerceViaIO`          |
//! | `Array`                        | `ArrayExpr`            |

use std::collections::HashMap;
use std::ffi::CString;

use pgrx::pg_sys;

use ra_core::expr::{BinOp, Const as RaConst, Expr as RaExpr, UnaryOp};
use ra_core::algebra::RelExpr;

/// Context for expression translation, carrying OID resolution maps.
pub struct ExprContext {
    /// Maps table name (lowercase) → 1-based range-table index.
    pub rtindex_map: HashMap<String, pg_sys::Index>,
    /// Maps table name (lowercase) → relation OID.
    pub rtoid_map: HashMap<String, pg_sys::Oid>,
    /// Pre-built `SubPlan` expression for each scalar sub-query, keyed by the
    /// address of its inner `RelExpr` (populated by the plan builder before
    /// expression translation). Lets a `SubQuery` expression resolve to its
    /// already-constructed SubPlan node.
    pub subplans: std::cell::RefCell<HashMap<usize, *mut pg_sys::Expr>>,
    /// Active CTE output-column scope for recursive-CTE recursive-term / body
    /// translation (columns resolve to a WorkTableScan / CteScan, not catalog).
    pub cte_scope: std::cell::RefCell<Option<CteScope>>,
    /// Active subquery output-column scope: when building a SubqueryScan over
    /// an inlined derived table (whose computed output columns have no catalog
    /// entry), columns resolve to `Var(rtindex, position)` of the subquery RTE
    /// by name. Used for aggregating/computing derived tables.
    pub subquery_scope: std::cell::RefCell<Option<SubqueryScope>>,
    /// Active join-side CTE scopes, keyed by lower-cased CTE reference name.
    /// When a non-recursive CTE is referenced as a join side it is built as a
    /// SubqueryScan over its definition; a qualified column `a.col` resolves to
    /// `Var(rtindex, position)` of that CTE's scan. A map (not a single scope)
    /// because a join can reference several CTEs at once.
    pub cte_join_scope: std::cell::RefCell<std::collections::HashMap<String, CteScope>>,
    /// Active correlation parameters for a LATERAL / correlated inner subquery
    /// built as the inner of a parameterized nested loop. Keyed by
    /// `(lower(qualifier), lower(column))` of the OUTER column referenced
    /// inside the inner; resolves to a PARAM_EXEC Param fed by nestParams.
    pub correlation_scope:
        std::cell::RefCell<std::collections::HashMap<(String, String), CorrParam>>,
}

/// A correlation parameter: the PARAM_EXEC id and type of an outer column
/// referenced inside a LATERAL / correlated inner subquery.
#[derive(Clone, Copy)]
pub struct CorrParam {
    pub paramid: i32,
    pub typ: pg_sys::Oid,
    pub typmod: i32,
    pub coll: pg_sys::Oid,
}

/// One output column of an inlined subquery/derived table.
pub struct SubqueryCol {
    /// Lower-cased output column name (or alias).
    pub name: String,
    pub typ: pg_sys::Oid,
    pub typmod: i32,
    pub coll: pg_sys::Oid,
}

/// The output columns of an in-scope derived table and the range-table index
/// of its subquery RTE (the `SubqueryScan`'s `scanrelid`).
pub struct SubqueryScope {
    pub rtindex: pg_sys::Index,
    pub cols: Vec<SubqueryCol>,
}

/// One output column of a CTE (no catalog entry; types come from the RTE_CTE).
pub struct CteCol {
    /// Lower-cased column name.
    pub name: String,
    pub typ: pg_sys::Oid,
    pub typmod: i32,
    pub coll: pg_sys::Oid,
}

/// The output columns of an in-scope CTE and the range-table index of the
/// scan (WorkTableScan or CteScan) that produces them.
pub struct CteScope {
    /// CTE reference name (lower-cased); column resolution only applies to
    /// unqualified columns or those qualified with this name.
    pub name: String,
    pub rtindex: pg_sys::Index,
    pub cols: Vec<CteCol>,
}

/// Translate a Ra [`RaExpr`] to a PostgreSQL `Expr*` node.
///
/// Returns `null_mut()` for unsupported expression types.
///
/// # Safety
///
/// Must be called from within a live PostgreSQL backend process.
pub unsafe fn translate(expr: &RaExpr, ctx: &ExprContext) -> *mut pg_sys::Expr {
    match expr {
        RaExpr::Const(c) => const_to_pg(c),
        RaExpr::Column(col_ref) => column_to_var(col_ref, ctx),
        RaExpr::BinOp { op, left, right } => match op {
            BinOp::And => build_bool_expr(pg_sys::BoolExprType::AND_EXPR, left, right, ctx),
            BinOp::Or => build_bool_expr(pg_sys::BoolExprType::OR_EXPR, left, right, ctx),
            _ => build_op_expr(op, left, right, ctx),
        },
        RaExpr::UnaryOp { op, operand } => match op {
            UnaryOp::Not => build_not(operand, ctx),
            UnaryOp::IsNull => build_null_test(operand, pg_sys::NullTestType::IS_NULL, ctx),
            UnaryOp::IsNotNull => build_null_test(operand, pg_sys::NullTestType::IS_NOT_NULL, ctx),
            UnaryOp::Neg => build_unary_neg(operand, ctx),
        },
        RaExpr::Function { name, args } => match (name.as_str(), args.as_slice()) {
            // Parser markers for IS NULL / IS NOT NULL (see ra_sql.lime).
            ("__is_null", [operand]) => {
                build_null_test(operand, pg_sys::NullTestType::IS_NULL, ctx)
            }
            ("__is_not_null", [operand]) => {
                build_null_test(operand, pg_sys::NullTestType::IS_NOT_NULL, ctx)
            }
            // LIKE / ILIKE parser markers → the `~~` / `~~*` text operators.
            ("__like", [l, r]) => build_named_op("~~", l, r, ctx),
            ("__ilike", [l, r]) => build_named_op("~~*", l, r, ctx),
            // IN (list): __in_list(test, v1, v2, ...) → test = ANY(ARRAY[...]).
            ("__in_list", args) if args.len() >= 2 => build_in_list(args, ctx),
            // `expr OP ANY/ALL (array)` → ScalarArrayOpExpr.
            (n, [test, arr]) if n.starts_with("__saoarr_") => build_sao_array(n, test, arr, ctx),
            // COALESCE(a, b, ...) → CoalesceExpr (not a catalog function).
            ("coalesce" | "COALESCE", args) if !args.is_empty() => build_coalesce(args, ctx),
            // NULLIF(a, b) → NullIfExpr (an OpExpr tagged T_NullIfExpr).
            ("nullif" | "NULLIF", [a, b]) => build_nullif(a, b, ctx),
            // GREATEST / LEAST → MinMaxExpr.
            ("greatest" | "GREATEST", args) if !args.is_empty() => {
                build_minmax(pg_sys::MinMaxOp::IS_GREATEST, args, ctx)
            }
            ("least" | "LEAST", args) if !args.is_empty() => {
                build_minmax(pg_sys::MinMaxOp::IS_LEAST, args, ctx)
            }
            _ => build_func_expr(name, args, ctx),
        },
        RaExpr::Case {
            operand,
            when_clauses,
            else_result,
        } => build_case_expr(
            operand.as_deref(),
            when_clauses,
            else_result.as_deref(),
            ctx,
        ),
        RaExpr::Cast {
            expr: inner,
            target_type,
        } => build_cast(inner, target_type, ctx),
        RaExpr::Array(elements) => build_array_expr(elements, ctx),
        // Scalar sub-query: return the SubPlan the plan builder prepared for
        // this inner query (keyed by its address); null if none → fallback.
        RaExpr::SubQuery { query, .. } => ctx
            .subplans
            .borrow()
            .get(&(std::ptr::from_ref::<RelExpr>(query.as_ref()) as usize))
            .copied()
            .unwrap_or(std::ptr::null_mut()),
        // Unsupported: FullTextMatch, VectorDistance, Pattern*, ArraySlice, etc.
        _ => std::ptr::null_mut(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Translate a Ra `Const` to a PostgreSQL `Const` node.
unsafe fn const_to_pg(c: &RaConst) -> *mut pg_sys::Expr {
    let node = alloc::<pg_sys::Const>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_Const;

    match c {
        RaConst::Null => {
            (*node).consttype = pg_sys::TEXTOID;
            (*node).consttypmod = -1;
            (*node).constlen = -1;
            (*node).constisnull = true;
            (*node).constbyval = false;
        }
        RaConst::Bool(b) => {
            (*node).consttype = pg_sys::BOOLOID;
            (*node).consttypmod = -1;
            (*node).constlen = 1;
            (*node).constvalue = pg_sys::Datum::from(*b as i32);
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::Int(i) => {
            // Use INT4 for values that fit, INT8 otherwise
            if *i >= i64::from(i32::MIN) && *i <= i64::from(i32::MAX) {
                (*node).consttype = pg_sys::INT4OID;
                (*node).consttypmod = -1;
                (*node).constlen = 4;
                (*node).constvalue = pg_sys::Datum::from(*i as i32);
            } else {
                (*node).consttype = pg_sys::INT8OID;
                (*node).consttypmod = -1;
                (*node).constlen = 8;
                (*node).constvalue = pg_sys::Datum::from(*i);
            }
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::Float(f) => {
            (*node).consttype = pg_sys::FLOAT8OID;
            (*node).consttypmod = -1;
            (*node).constlen = 8;
            // Float8GetDatum: reinterpret f64 bits as Datum (pass-by-val on 64-bit)
            (*node).constvalue = pg_sys::Datum::from((*f).to_bits() as usize);
            (*node).constisnull = false;
            (*node).constbyval = true;
        }
        RaConst::String(s) => {
            if let Ok(cs) = CString::new(s.as_str()) {
                (*node).consttype = pg_sys::TEXTOID;
                (*node).consttypmod = -1;
                (*node).constlen = -1;
                let text_ptr = pg_sys::cstring_to_text(cs.as_ptr());
                (*node).constvalue = pg_sys::Datum::from(text_ptr as usize);
                (*node).constisnull = false;
                (*node).constbyval = false;
            } else {
                (*node).constisnull = true;
            }
        }
    }
    // Collatable constants (text) need a collation so collation-sensitive
    // operators can resolve; non-collatable types get InvalidOid.
    (*node).constcollid = pg_sys::get_typcollation((*node).consttype);
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Column reference → Var
// ─────────────────────────────────────────────────────────────────────────────

/// Translate a Ra `ColumnRef` to a PostgreSQL `Var` node.
unsafe fn column_to_var(col: &ra_core::expr::ColumnRef, ctx: &ExprContext) -> *mut pg_sys::Expr {
    let table = col.table.as_deref().unwrap_or("").to_lowercase();
    let col_name = match CString::new(col.column.as_str()) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };

    // Subquery output-column resolution: when building a Filter/Project over an
    // inlined derived table, a column refers to that subquery's computed output
    // (no catalog entry). Resolve it to an OUTER_VAR reference into the child
    // plan's output by name. Matched by column name (the subquery is the sole
    // input at this level); a derived-table qualifier (`t.s`) is accepted.
    if let Some(scope) = ctx.subquery_scope.borrow().as_ref() {
        let want = col.column.to_lowercase();
        for (i, c) in scope.cols.iter().enumerate() {
            if c.name == want {
                let var = alloc::<pg_sys::Var>();
                (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
                (*var).varno = scope.rtindex as i32;
                (*var).varattno = (i + 1) as i16;
                (*var).vartype = c.typ;
                (*var).vartypmod = c.typmod;
                (*var).varcollid = c.coll;
                (*var).varlevelsup = 0;
                (*var).location = -1;
                return var.cast();
            }
        }
    }

    // CTE-column resolution: when building a recursive CTE's recursive term
    // or body, columns refer to the CTE's output (a WorkTableScan / CteScan),
    // which has no catalog entry. Resolve such names against the CTE scope —
    // but only unqualified columns or those qualified with the CTE's name, so
    // a same-named column of a joined base relation (e.g. `labels.id` vs the
    // CTE's `id`) is not mis-resolved to the CTE.
    if let Some(scope) = ctx.cte_scope.borrow().as_ref() {
        let qualifies = col
            .table
            .as_deref()
            .is_none_or(|t| t.eq_ignore_ascii_case(&scope.name));
        if qualifies {
            let want = col.column.to_lowercase();
            for (i, c) in scope.cols.iter().enumerate() {
                if c.name == want {
                    let var = alloc::<pg_sys::Var>();
                    (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
                    (*var).varno = scope.rtindex as i32;
                    (*var).varattno = (i + 1) as i16;
                    (*var).vartype = c.typ;
                    (*var).vartypmod = c.typmod;
                    (*var).varcollid = c.coll;
                    (*var).varlevelsup = 0;
                    (*var).location = -1;
                    return var.cast();
                }
            }
        }
    }

    // Correlation parameter resolution: inside a LATERAL / correlated inner
    // subquery, a column qualified with the outer relation resolves to a
    // PARAM_EXEC Param (fed by the nested loop's nestParams), not a Var.
    if let Some(t) = col.table.as_deref() {
        let key = (t.to_lowercase(), col.column.to_lowercase());
        if let Some(cp) = ctx.correlation_scope.borrow().get(&key) {
            let p = alloc::<pg_sys::Param>();
            (*p).xpr.type_ = pg_sys::NodeTag::T_Param;
            (*p).paramkind = pg_sys::ParamKind::PARAM_EXEC;
            (*p).paramid = cp.paramid;
            (*p).paramtype = cp.typ;
            (*p).paramtypmod = cp.typmod;
            (*p).paramcollid = cp.coll;
            (*p).location = -1;
            return p.cast();
        }
    }

    // Join-side CTE resolution: a column qualified with a join-referenced
    // CTE's name (`a.col`) resolves to that CTE's SubqueryScan output. Keyed
    // by qualifier, so only an explicitly `a.`-qualified column matches — an
    // unqualified column of the CTE's own definition is left to catalog
    // resolution and never mis-bound here.
    if let Some(t) = col.table.as_deref() {
        let scopes = ctx.cte_join_scope.borrow();
        if let Some(scope) = scopes.get(&t.to_lowercase()) {
            let want = col.column.to_lowercase();
            for (i, c) in scope.cols.iter().enumerate() {
                if c.name == want {
                    let var = alloc::<pg_sys::Var>();
                    (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
                    (*var).varno = scope.rtindex as i32;
                    (*var).varattno = (i + 1) as i16;
                    (*var).vartype = c.typ;
                    (*var).vartypmod = c.typmod;
                    (*var).varcollid = c.coll;
                    (*var).varlevelsup = 0;
                    (*var).location = -1;
                    return var.cast();
                }
            }
        }
    }

    // Resolve the column to (rtindex, reloid, attnum). A qualified
    // reference (`t.col`) looks up its table directly. An UNqualified
    // reference (`col`) — which Ra's Lime parser emits for
    // single-table `SELECT col FROM t` — is resolved by finding the
    // range-table relation that actually has a column of this name
    // (mirroring PostgreSQL's own unqualified-name resolution). Without
    // this, unqualified columns produced NULL Vars, silently dropping
    // targetlist entries and filter quals.
    let resolve = |rtindex: pg_sys::Index, reloid: pg_sys::Oid| -> Option<(pg_sys::Index, pg_sys::Oid, i16)> {
        let attnum = pg_sys::get_attnum(reloid, col_name.as_ptr());
        if attnum == pg_sys::InvalidAttrNumber as i16 {
            None
        } else {
            Some((rtindex, reloid, attnum))
        }
    };
    let (rtindex, reloid, attnum) = if table.is_empty() {
        // Unqualified: find the relation owning this column. When several
        // range-table entries own a same-named column (e.g. a self-correlated
        // subquery pulls a second `orders` instance into the range table), the
        // outer query's unqualified column belongs to the outer relation, which
        // is assigned the lower rtindex — pulled-up subquery relations are
        // appended after it. Pick the lowest owning rtindex deterministically
        // (a plain HashMap scan would pick an arbitrary instance and emit a Var
        // whose varno mismatches its scan's scanrelid).
        let mut best: Option<(pg_sys::Index, pg_sys::Oid, i16)> = None;
        for (t, &rtindex) in &ctx.rtindex_map {
            if let Some(&reloid) = ctx.rtoid_map.get(t) {
                if let Some(r) = resolve(rtindex, reloid) {
                    if best.is_none_or(|(b, _, _)| rtindex < b) {
                        best = Some(r);
                    }
                }
            }
        }
        match best {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        }
    } else {
        let rtindex = match ctx.rtindex_map.get(&table) {
            Some(&idx) => idx,
            None => return std::ptr::null_mut(),
        };
        let reloid = match ctx.rtoid_map.get(&table) {
            Some(&oid) => oid,
            None => return std::ptr::null_mut(),
        };
        match resolve(rtindex, reloid) {
            Some(r) => r,
            None => return std::ptr::null_mut(),
        }
    };

    let atttype = pg_sys::get_atttype(reloid, attnum);
    if atttype == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    // get_atttypmod is not exposed by pgrx; use -1 (unspecified typmod).
    // For columns with explicit typemod (e.g., varchar(255)), the executor
    // will still function correctly since the type itself carries the info.
    let atttypmod: i32 = -1;

    let var = alloc::<pg_sys::Var>();
    (*var).xpr.type_ = pg_sys::NodeTag::T_Var;
    (*var).varno = rtindex as i32;
    (*var).varattno = attnum;
    (*var).vartype = atttype;
    (*var).vartypmod = atttypmod;
    // Collation is required for collation-sensitive operators (text
    // comparison/sort); a missing collation makes the executor raise
    // "could not determine which collation to use". Use the type's default
    // collation (correct for columns without an explicit COLLATE clause).
    (*var).varcollid = pg_sys::get_typcollation(atttype);
    (*var).varlevelsup = 0;
    (var as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Binary operators → OpExpr
// ─────────────────────────────────────────────────────────────────────────────

/// Translate a binary comparison/arithmetic op to a PostgreSQL `OpExpr`.
unsafe fn build_op_expr(
    op: &BinOp,
    left: &RaExpr,
    right: &RaExpr,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let Some(op_str) = binop_op_str(op) else {
        return std::ptr::null_mut();
    };
    build_named_op(op_str, left, right, ctx)
}

/// Build an `OpExpr` for a named binary operator (`op_str`, e.g. `=`, `~~`)
/// over two Ra operands, resolving the operator from the operand types.
unsafe fn build_named_op(
    op_str: &str,
    left: &RaExpr,
    right: &RaExpr,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let left_pg = translate(left, ctx);
    let right_pg = translate(right, ctx);
    op_expr_from_nodes(op_str, left_pg, right_pg)
}

/// String operator name for a [`BinOp`] (`None` for non-operator variants).
#[must_use]
pub fn binop_op_str(op: &BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Eq => "=",
        BinOp::Ne => "<>",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Concat => "||",
        _ => return None,
    })
}

/// Build an `OpExpr` for `op_str` over two already-translated operand nodes.
/// Used both for Ra-operand binops and for aggregate-output expressions whose
/// operands are pre-built `Aggref` / `Var` nodes.
pub(crate) unsafe fn op_expr_from_nodes(
    op_str: &str,
    left_pg: *mut pg_sys::Expr,
    right_pg: *mut pg_sys::Expr,
) -> *mut pg_sys::Expr {
    if left_pg.is_null() || right_pg.is_null() {
        return std::ptr::null_mut();
    }

    let op_cstr = match CString::new(op_str) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let op_string_node = pg_sys::makeString(op_cstr.as_ptr().cast_mut());
    let opname_list = pg_sys::lappend(std::ptr::null_mut(), op_string_node.cast());

    let left_type = expr_result_type(left_pg);
    let right_type = expr_result_type(right_pg);

    let opno = pg_sys::OpernameGetOprid(opname_list, left_type, right_type);
    if opno == pg_sys::InvalidOid {
        // No operator for these exact types (e.g. `int4 < numeric`, as produced
        // by comparing an int column to `avg(...)`, or `bpchar = text`). Defer
        // to PostgreSQL's own operator resolution, which selects the best
        // candidate and inserts the same implicit coercions the parser would.
        // Returns a fully-formed OpExpr with coerced arguments.
        let op = pg_sys::make_op(
            std::ptr::null_mut(),
            opname_list,
            left_pg.cast(),
            right_pg.cast(),
            std::ptr::null_mut(),
            -1,
        );
        // make_op does NOT assign collations — PG's parser does that in a
        // separate pass (assign_expr_collations) that Ra otherwise skips. Run
        // it here so collation-sensitive coercions (e.g. bpchar→text comparison)
        // carry a valid inputcollid; without it the executor raises "could not
        // determine which collation to use for string comparison".
        if !op.is_null() {
            pg_sys::assign_expr_collations(std::ptr::null_mut(), op.cast());
        }
        return op;
    }

    let opfuncid = pg_sys::get_opcode(opno);
    let opresulttype = pg_sys::get_func_rettype(opfuncid);

    let node = alloc::<pg_sys::OpExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_OpExpr;
    (*node).opno = opno;
    (*node).opfuncid = opfuncid;
    (*node).opresulttype = opresulttype;
    (*node).opretset = false;
    // Derive the input collation from the operands (PG requires it for
    // collation-sensitive operators such as text comparison). The result
    // collation applies only when the result type is itself collatable.
    let mut inputcollid = pg_sys::exprCollation(left_pg.cast());
    if inputcollid == pg_sys::InvalidOid {
        inputcollid = pg_sys::exprCollation(right_pg.cast());
    }
    (*node).inputcollid = inputcollid;
    (*node).opcollid = if pg_sys::get_typcollation(opresulttype) == pg_sys::InvalidOid {
        pg_sys::InvalidOid
    } else {
        inputcollid
    };
    let mut args = std::ptr::null_mut::<pg_sys::List>();
    args = pg_sys::lappend(args, left_pg.cast());
    args = pg_sys::lappend(args, right_pg.cast());
    (*node).args = args;
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Boolean operators → BoolExpr
// ─────────────────────────────────────────────────────────────────────────────

/// Build a PostgreSQL `BoolExpr` (AND/OR) from two Ra expressions.
unsafe fn build_bool_expr(
    bool_type: pg_sys::BoolExprType::Type,
    left: &RaExpr,
    right: &RaExpr,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let left_pg = translate(left, ctx);
    let right_pg = translate(right, ctx);
    bool_expr_from_nodes(bool_type, left_pg, right_pg)
}

/// Build a 2-arg `BoolExpr` from already-translated operand nodes.
pub(crate) unsafe fn bool_expr_from_nodes(
    bool_type: pg_sys::BoolExprType::Type,
    left_pg: *mut pg_sys::Expr,
    right_pg: *mut pg_sys::Expr,
) -> *mut pg_sys::Expr {
    if left_pg.is_null() || right_pg.is_null() {
        return std::ptr::null_mut();
    }
    let node = alloc::<pg_sys::BoolExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_BoolExpr;
    (*node).boolop = bool_type;
    let mut args = std::ptr::null_mut::<pg_sys::List>();
    args = pg_sys::lappend(args, left_pg.cast());
    args = pg_sys::lappend(args, right_pg.cast());
    (*node).args = args;
    (node as *mut pg_sys::Expr)
}

/// Build NOT expression.
unsafe fn build_not(operand: &RaExpr, ctx: &ExprContext) -> *mut pg_sys::Expr {
    let arg = translate(operand, ctx);
    if arg.is_null() {
        return std::ptr::null_mut();
    }
    let node = alloc::<pg_sys::BoolExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_BoolExpr;
    (*node).boolop = pg_sys::BoolExprType::NOT_EXPR;
    (*node).args = pg_sys::lappend(std::ptr::null_mut(), arg.cast());
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Null test
// ─────────────────────────────────────────────────────────────────────────────

/// Build a PostgreSQL `NullTest` node (IS NULL / IS NOT NULL).
unsafe fn build_null_test(
    operand: &RaExpr,
    test_type: pg_sys::NullTestType::Type,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let arg = translate(operand, ctx);
    if arg.is_null() {
        return std::ptr::null_mut();
    }
    let node = alloc::<pg_sys::NullTest>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_NullTest;
    (*node).arg = arg.cast();
    (*node).nulltesttype = test_type;
    (*node).argisrow = false;
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Unary negation
// ─────────────────────────────────────────────────────────────────────────────

/// Build a unary negation `OpExpr` (`-x`).
unsafe fn build_unary_neg(operand: &RaExpr, ctx: &ExprContext) -> *mut pg_sys::Expr {
    let arg = translate(operand, ctx);
    if arg.is_null() {
        return std::ptr::null_mut();
    }
    let arg_type = expr_result_type(arg);
    let op_cstr = c"-";
    let op_node = pg_sys::makeString(op_cstr.as_ptr().cast_mut());
    let opname = pg_sys::lappend(std::ptr::null_mut(), op_node.cast());
    let opno = pg_sys::OpernameGetOprid(opname, arg_type, pg_sys::InvalidOid);
    if opno == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let opfuncid = pg_sys::get_opcode(opno);
    let opresulttype = pg_sys::get_func_rettype(opfuncid);
    let node = alloc::<pg_sys::OpExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_OpExpr;
    (*node).opno = opno;
    (*node).opfuncid = opfuncid;
    (*node).opresulttype = opresulttype;
    (*node).args = pg_sys::lappend(std::ptr::null_mut(), arg.cast());
    (node as *mut pg_sys::Expr)
}

/// Build a `CoalesceExpr` from COALESCE arguments. To guarantee a correct
/// executor result without PG's parse-analysis type unification, only build
/// when every argument has the same result type (the common case); otherwise
/// return null so the planner hook defers to native PG.
/// Build `test = ANY(ARRAY[values...])` for `IN (list)` from the marker
/// `__in_list(test, v1, v2, ...)`.
/// `expr OP ANY/ALL (array)` from a `__saoarr_<op>_<any|all>` marker →
/// `ScalarArrayOpExpr`. The array operand is translated as-is (e.g. an
/// `ARRAY[...]` literal); the operator compares the test value to each element.
unsafe fn build_sao_array(
    name: &str,
    test_e: &RaExpr,
    arr_e: &RaExpr,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let suffix = &name["__saoarr_".len()..];
    let (op_tok, use_or) = match suffix.rsplit_once('_') {
        Some((op, "any")) => (op, true),
        Some((op, "all")) => (op, false),
        _ => return std::ptr::null_mut(),
    };
    let op = match op_tok {
        "eq" => "=",
        "ne" => "<>",
        "lt" => "<",
        "le" => "<=",
        "gt" => ">",
        "ge" => ">=",
        _ => return std::ptr::null_mut(),
    };
    let test = translate(test_e, ctx);
    let arr = translate(arr_e, ctx);
    if test.is_null() || arr.is_null() {
        return std::ptr::null_mut();
    }
    let elem_type = pg_sys::get_element_type(pg_sys::exprType(arr.cast()));
    if elem_type == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let opc = match CString::new(op) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let opname = pg_sys::lappend(
        std::ptr::null_mut(),
        pg_sys::makeString(opc.as_ptr().cast_mut()).cast(),
    );
    let opno = pg_sys::OpernameGetOprid(opname, pg_sys::exprType(test.cast()), elem_type);
    if opno == pg_sys::InvalidOid {
        // No exact operator for these types: defer to PostgreSQL's parser
        // helper, which coerces the array elements and applies useOr like the
        // parser, then assign collations for collation-sensitive comparisons.
        let sao = pg_sys::make_scalar_array_op(
            std::ptr::null_mut(),
            opname,
            use_or,
            test.cast(),
            arr.cast(),
            -1,
        );
        if !sao.is_null() {
            pg_sys::assign_expr_collations(std::ptr::null_mut(), sao.cast());
        }
        return sao;
    }
    let node = alloc::<pg_sys::ScalarArrayOpExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_ScalarArrayOpExpr;
    (*node).opno = opno;
    (*node).opfuncid = pg_sys::get_opcode(opno);
    (*node).hashfuncid = pg_sys::InvalidOid;
    (*node).negfuncid = pg_sys::InvalidOid;
    (*node).useOr = use_or;
    let mut coll = pg_sys::exprCollation(test.cast());
    if coll == pg_sys::InvalidOid {
        coll = pg_sys::exprCollation(arr.cast());
    }
    (*node).inputcollid = coll;
    (*node).args = pg_sys::lappend(pg_sys::lappend(std::ptr::null_mut(), test.cast()), arr.cast());
    (*node).location = -1;
    node.cast()
}

unsafe fn build_in_list(args: &[RaExpr], ctx: &ExprContext) -> *mut pg_sys::Expr {
    let test = translate(&args[0], ctx);
    if test.is_null() {
        return std::ptr::null_mut();
    }
    let arr = build_array_expr(&args[1..], ctx);
    if arr.is_null() {
        return std::ptr::null_mut();
    }
    let test_type = pg_sys::exprType(test.cast());
    let elem_type = (*arr.cast::<pg_sys::ArrayExpr>()).element_typeid;
    let eq = match CString::new("=") {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let opname = pg_sys::lappend(
        std::ptr::null_mut(),
        pg_sys::makeString(eq.as_ptr().cast_mut()).cast(),
    );
    let opno = pg_sys::OpernameGetOprid(opname, test_type, elem_type);
    if opno == pg_sys::InvalidOid {
        // No exact `test_type = elem_type` operator (e.g. `bpchar = text` /
        // `varchar = text` from a text-typed IN list). Defer to PostgreSQL's
        // parser helper, which coerces the array elements to the operator's
        // input type and sets useOr exactly like transformAExprIn, then assign
        // collations so text comparisons carry a valid collation.
        let sao = pg_sys::make_scalar_array_op(
            std::ptr::null_mut(),
            opname,
            true,
            test.cast(),
            arr.cast(),
            -1,
        );
        if !sao.is_null() {
            pg_sys::assign_expr_collations(std::ptr::null_mut(), sao.cast());
        }
        return sao;
    }
    let node = alloc::<pg_sys::ScalarArrayOpExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_ScalarArrayOpExpr;
    (*node).opno = opno;
    (*node).opfuncid = pg_sys::get_opcode(opno);
    (*node).hashfuncid = pg_sys::InvalidOid;
    (*node).negfuncid = pg_sys::InvalidOid;
    (*node).useOr = true;
    let mut coll = pg_sys::exprCollation(test.cast());
    if coll == pg_sys::InvalidOid {
        coll = pg_sys::exprCollation(arr.cast());
    }
    (*node).inputcollid = coll;
    (*node).args = pg_sys::lappend(pg_sys::lappend(std::ptr::null_mut(), test.cast()), arr.cast());
    (*node).location = -1;
    node.cast()
}

/// Build `NULLIF(a, b)` as a `NullIfExpr` (an `OpExpr` tagged T_NullIfExpr):
/// returns NULL when `a = b`, else `a`. Result type is `a`'s type.
unsafe fn build_nullif(a: &RaExpr, b: &RaExpr, ctx: &ExprContext) -> *mut pg_sys::Expr {
    let l = translate(a, ctx);
    let r = translate(b, ctx);
    if l.is_null() || r.is_null() {
        return std::ptr::null_mut();
    }
    let lt = pg_sys::exprType(l.cast());
    let eq = match CString::new("=") {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let opname = pg_sys::lappend(
        std::ptr::null_mut(),
        pg_sys::makeString(eq.as_ptr().cast_mut()).cast(),
    );
    let opno = pg_sys::OpernameGetOprid(opname, lt, pg_sys::exprType(r.cast()));
    if opno == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let node = alloc::<pg_sys::NullIfExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_NullIfExpr;
    (*node).opno = opno;
    (*node).opfuncid = pg_sys::get_opcode(opno);
    (*node).opresulttype = lt;
    (*node).opretset = false;
    let coll = pg_sys::exprCollation(l.cast());
    (*node).inputcollid = coll;
    (*node).opcollid = if pg_sys::get_typcollation(lt) == pg_sys::InvalidOid {
        pg_sys::InvalidOid
    } else {
        coll
    };
    (*node).args = pg_sys::lappend(pg_sys::lappend(std::ptr::null_mut(), l.cast()), r.cast());
    (*node).location = -1;
    node.cast()
}

/// Build `GREATEST` / `LEAST` as a `MinMaxExpr`. Only when all arguments share
/// a result type (avoiding PG's parse-time type unification); else defers.
unsafe fn build_minmax(
    op: pg_sys::MinMaxOp::Type,
    args: &[RaExpr],
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let mut pg_args: *mut pg_sys::List = std::ptr::null_mut();
    let mut ty = pg_sys::InvalidOid;
    let mut coll = pg_sys::InvalidOid;
    for a in args {
        let p = translate(a, ctx);
        if p.is_null() {
            return std::ptr::null_mut();
        }
        let t = pg_sys::exprType(p.cast());
        if ty == pg_sys::InvalidOid {
            ty = t;
            coll = pg_sys::exprCollation(p.cast());
        } else if t != ty {
            return std::ptr::null_mut();
        }
        pg_args = pg_sys::lappend(pg_args, p.cast());
    }
    let node = alloc::<pg_sys::MinMaxExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_MinMaxExpr;
    (*node).minmaxtype = ty;
    (*node).minmaxcollid = if pg_sys::get_typcollation(ty) == pg_sys::InvalidOid {
        pg_sys::InvalidOid
    } else {
        coll
    };
    (*node).inputcollid = coll;
    (*node).op = op;
    (*node).args = pg_args;
    (*node).location = -1;
    node.cast()
}

unsafe fn build_coalesce(args: &[RaExpr], ctx: &ExprContext) -> *mut pg_sys::Expr {
    let mut pg_args: *mut pg_sys::List = std::ptr::null_mut();
    for a in args {
        let pg = translate(a, ctx);
        if pg.is_null() {
            return std::ptr::null_mut();
        }
        pg_args = pg_sys::lappend(pg_args, pg.cast());
    }
    if pg_args.is_null() {
        return std::ptr::null_mut();
    }

    // Determine the common type PostgreSQL would use across the arguments and
    // coerce each to it (mirrors transformCoalesceExpr). This handles mixed
    // but compatible types like `varchar` column + `unknown`/`text` literal —
    // requiring an exact type match previously rejected those.
    let ctx_cstr = match CString::new("COALESCE") {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };
    let mut which: *mut pg_sys::Node = std::ptr::null_mut();
    let common_type = pg_sys::select_common_type(
        std::ptr::null_mut(),
        pg_args,
        ctx_cstr.as_ptr(),
        &mut which,
    );
    if common_type == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }

    let mut coerced: *mut pg_sys::List = std::ptr::null_mut();
    let n = pg_sys::list_length(pg_args);
    for i in 0..n {
        let node = pg_sys::list_nth(pg_args, i).cast::<pg_sys::Node>();
        let c = pg_sys::coerce_to_common_type(
            std::ptr::null_mut(),
            node,
            common_type,
            ctx_cstr.as_ptr(),
        );
        if c.is_null() {
            return std::ptr::null_mut();
        }
        coerced = pg_sys::lappend(coerced, c.cast());
    }

    let node = alloc::<pg_sys::CoalesceExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_CoalesceExpr;
    (*node).coalescetype = common_type;
    (*node).args = coerced;
    (*node).location = -1;
    // Assign collation from the (coerced) arguments — PG's parser does this in
    // a separate pass that Ra otherwise skips; without it a text/varchar
    // COALESCE can raise "could not determine which collation to use".
    pg_sys::assign_expr_collations(std::ptr::null_mut(), node.cast());
    node.cast()
}

// ─────────────────────────────────────────────────────────────────────────────
// Function calls → FuncExpr
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `FuncExpr` from a function name and argument list.
///
/// Resolves the function OID from `pg_proc` using the function name and
/// argument types.
unsafe fn build_func_expr(name: &str, args: &[RaExpr], ctx: &ExprContext) -> *mut pg_sys::Expr {
    // Translate all arguments first to determine arg types.
    let mut pg_args = std::ptr::null_mut::<pg_sys::List>();
    let mut arg_types = Vec::with_capacity(args.len());

    for arg in args {
        let pg_arg = translate(arg, ctx);
        if pg_arg.is_null() {
            return std::ptr::null_mut();
        }
        arg_types.push(expr_result_type(pg_arg));
        pg_args = pg_sys::lappend(pg_args, pg_arg.cast());
    }

    // Resolve function OID via catalog lookup.
    let func_name = match CString::new(name.to_lowercase()) {
        Ok(cs) => cs,
        Err(_) => return std::ptr::null_mut(),
    };
    let name_node = pg_sys::makeString(func_name.as_ptr().cast_mut());
    let funcname_list = pg_sys::lappend(std::ptr::null_mut(), name_node.cast());

    let funcoid = pg_sys::LookupFuncName(
        funcname_list,
        arg_types.len() as i32,
        arg_types.as_ptr(),
        true, // missing_ok
    );
    if funcoid == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }

    let rettype = pg_sys::get_func_rettype(funcoid);

    // Derive the input collation from the arguments (PG requires it for
    // collation-sensitive functions such as upper()/lower()). The result
    // collation applies only when the result type is itself collatable.
    let mut inputcollid = pg_sys::InvalidOid;
    {
        let elems = if pg_args.is_null() {
            std::ptr::null()
        } else {
            (*pg_args).elements
        };
        let n = if pg_args.is_null() { 0 } else { (*pg_args).length };
        for i in 0..n {
            let a = (*elems.add(i as usize)).ptr_value.cast::<pg_sys::Expr>();
            let c = pg_sys::exprCollation(a.cast());
            if c != pg_sys::InvalidOid {
                inputcollid = c;
                break;
            }
        }
    }
    let funccollid = if pg_sys::get_typcollation(rettype) == pg_sys::InvalidOid {
        pg_sys::InvalidOid
    } else {
        inputcollid
    };

    let node = alloc::<pg_sys::FuncExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_FuncExpr;
    (*node).funcid = funcoid;
    (*node).funcresulttype = rettype;
    (*node).funcretset = false;
    (*node).funcvariadic = false;
    (*node).funcformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CALL;
    (*node).funccollid = funccollid;
    (*node).inputcollid = inputcollid;
    (*node).args = pg_args;
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// CASE expression → CaseExpr
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `CaseExpr` from Ra's Case expression.
unsafe fn build_case_expr(
    _operand: Option<&RaExpr>,
    when_clauses: &[(RaExpr, RaExpr)],
    else_result: Option<&RaExpr>,
    ctx: &ExprContext,
) -> *mut pg_sys::Expr {
    let node = alloc::<pg_sys::CaseExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_CaseExpr;
    (*node).casetype = pg_sys::InvalidOid; // determined after building clauses
    (*node).casecollid = pg_sys::InvalidOid;
    (*node).arg = std::ptr::null_mut();

    // Build CaseWhen list
    let mut when_list = std::ptr::null_mut::<pg_sys::List>();
    let mut result_type = pg_sys::InvalidOid;

    for (condition, result) in when_clauses {
        let cond_pg = translate(condition, ctx);
        let result_pg = translate(result, ctx);
        if cond_pg.is_null() || result_pg.is_null() {
            continue;
        }

        if result_type == pg_sys::InvalidOid {
            result_type = expr_result_type(result_pg);
        }

        let when_node = alloc::<pg_sys::CaseWhen>();
        (*when_node).xpr.type_ = pg_sys::NodeTag::T_CaseWhen;
        (*when_node).expr = cond_pg.cast();
        (*when_node).result = result_pg.cast();
        when_list = pg_sys::lappend(when_list, when_node.cast());
    }

    (*node).args = when_list;

    // ELSE clause
    if let Some(else_expr) = else_result {
        let else_pg = translate(else_expr, ctx);
        if !else_pg.is_null() {
            (*node).defresult = else_pg.cast();
            if result_type == pg_sys::InvalidOid {
                result_type = expr_result_type(else_pg);
            }
        }
    }

    // Default to TEXT if we couldn't determine type
    if result_type == pg_sys::InvalidOid {
        result_type = pg_sys::TEXTOID;
    }
    (*node).casetype = result_type;

    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// CAST → CoerceViaIO
// ─────────────────────────────────────────────────────────────────────────────

/// Build a type coercion node.
///
/// Uses `CoerceViaIO` which works for any type with I/O functions.
unsafe fn build_cast(inner: &RaExpr, target_type: &str, ctx: &ExprContext) -> *mut pg_sys::Expr {
    let arg = translate(inner, ctx);
    if arg.is_null() {
        return std::ptr::null_mut();
    }

    let target_oid = resolve_type_oid(target_type);
    if target_oid == pg_sys::InvalidOid {
        return std::ptr::null_mut();
    }
    let source_oid = pg_sys::exprType(arg.cast());
    let source_coll = pg_sys::exprCollation(arg.cast());
    let target_coll = if pg_sys::get_typcollation(target_oid) == pg_sys::InvalidOid {
        pg_sys::InvalidOid
    } else {
        source_coll
    };

    // No-op cast (same type) → return the argument unchanged.
    if source_oid == target_oid {
        return arg;
    }

    // Resolve the correct coercion: a dedicated cast function (e.g.
    // bool→int4), a binary relabel (binary-compatible types), or text I/O.
    // Using CoerceViaIO unconditionally is wrong for function casts (e.g.
    // bool→int via I/O parses 'f' as an integer and errors).
    let mut castfunc = pg_sys::InvalidOid;
    let path = pg_sys::find_coercion_pathway(
        target_oid,
        source_oid,
        pg_sys::CoercionContext::COERCION_EXPLICIT,
        &mut castfunc,
    );
    match path {
        pg_sys::CoercionPathType::COERCION_PATH_FUNC if castfunc != pg_sys::InvalidOid => {
            let node = alloc::<pg_sys::FuncExpr>();
            (*node).xpr.type_ = pg_sys::NodeTag::T_FuncExpr;
            (*node).funcid = castfunc;
            (*node).funcresulttype = target_oid;
            (*node).funcretset = false;
            (*node).funcvariadic = false;
            (*node).funcformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CAST;
            (*node).funccollid = target_coll;
            (*node).inputcollid = source_coll;
            (*node).args = pg_sys::lappend(std::ptr::null_mut(), arg.cast());
            node.cast()
        }
        pg_sys::CoercionPathType::COERCION_PATH_RELABELTYPE => {
            let node = alloc::<pg_sys::RelabelType>();
            (*node).xpr.type_ = pg_sys::NodeTag::T_RelabelType;
            (*node).arg = arg.cast();
            (*node).resulttype = target_oid;
            (*node).resulttypmod = -1;
            (*node).resultcollid = target_coll;
            (*node).relabelformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CAST;
            node.cast()
        }
        pg_sys::CoercionPathType::COERCION_PATH_COERCEVIAIO => {
            let node = alloc::<pg_sys::CoerceViaIO>();
            (*node).xpr.type_ = pg_sys::NodeTag::T_CoerceViaIO;
            (*node).arg = arg.cast();
            (*node).resulttype = target_oid;
            (*node).resultcollid = target_coll;
            (*node).coerceformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CAST;
            node.cast()
        }
        // ARRAYCOERCE / NONE: defer to native PG.
        _ => std::ptr::null_mut(),
    }
}

/// Resolve a type name string to its OID.
unsafe fn resolve_type_oid(type_name: &str) -> pg_sys::Oid {
    // Map common SQL type names to PG type names
    let lower = type_name.to_lowercase();
    let pg_type = match lower.as_str() {
        "int" | "integer" | "int4" => "int4",
        "bigint" | "int8" => "int8",
        "smallint" | "int2" => "int2",
        "real" | "float4" => "float4",
        "double precision" | "float8" | "double" | "float" => "float8",
        "text" => "text",
        "varchar" | "character varying" => "varchar",
        "char" | "character" => "bpchar",
        "boolean" | "bool" => "bool",
        "date" => "date",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timestamptz" | "timestamp with time zone" => "timestamptz",
        "numeric" | "decimal" => "numeric",
        "json" => "json",
        "jsonb" => "jsonb",
        "uuid" => "uuid",
        "bytea" => "bytea",
        other => other,
    };

    let type_cstr = match CString::new(pg_type) {
        Ok(cs) => cs,
        Err(_) => return pg_sys::InvalidOid,
    };
    let type_name_node = pg_sys::makeString(type_cstr.as_ptr().cast_mut());
    let type_name_list = pg_sys::lappend(std::ptr::null_mut(), type_name_node.cast());

    pg_sys::LookupTypeNameOid(
        std::ptr::null_mut(), // pstate
        pg_sys::makeTypeNameFromNameList(type_name_list),
        true, // missing_ok
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Array expression → ArrayExpr
// ─────────────────────────────────────────────────────────────────────────────

/// Build an `ArrayExpr` from a list of element expressions.
unsafe fn build_array_expr(elements: &[RaExpr], ctx: &ExprContext) -> *mut pg_sys::Expr {
    let node = alloc::<pg_sys::ArrayExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_ArrayExpr;
    (*node).multidims = false;

    let mut elem_list = std::ptr::null_mut::<pg_sys::List>();
    let mut elem_type = pg_sys::InvalidOid;

    for elem in elements {
        let pg_elem = translate(elem, ctx);
        if pg_elem.is_null() {
            continue;
        }
        if elem_type == pg_sys::InvalidOid {
            elem_type = expr_result_type(pg_elem);
        }
        elem_list = pg_sys::lappend(elem_list, pg_elem.cast());
    }

    // Look up array type for this element type
    if elem_type != pg_sys::InvalidOid {
        (*node).element_typeid = elem_type;
        (*node).array_typeid = pg_sys::get_array_type(elem_type);
    } else {
        (*node).element_typeid = pg_sys::TEXTOID;
        (*node).array_typeid = pg_sys::get_array_type(pg_sys::TEXTOID);
    }
    (*node).array_collid = pg_sys::InvalidOid;
    (*node).elements = elem_list;
    (node as *mut pg_sys::Expr)
}

// ─────────────────────────────────────────────────────────────────────────────
// Utility
// ─────────────────────────────────────────────────────────────────────────────

/// Infer the result type OID of a PostgreSQL `Expr*` node.
pub unsafe fn expr_result_type(expr: *mut pg_sys::Expr) -> pg_sys::Oid {
    if expr.is_null() {
        return pg_sys::InvalidOid;
    }
    // PG's canonical accessor handles every Expr node kind (Aggref, Param,
    // RelabelType, ...) — a hand-rolled match misses types such as Aggref,
    // which broke operator resolution for expressions over aggregates.
    pg_sys::exprType(expr.cast())
}

/// Allocate a zeroed node in the current PostgreSQL memory context.
unsafe fn alloc<T>() -> *mut T {
    pg_sys::palloc0(std::mem::size_of::<T>()).cast()
}
