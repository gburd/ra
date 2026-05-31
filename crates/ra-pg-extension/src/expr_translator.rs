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

    // CTE-column resolution: when building a recursive CTE's recursive term
    // or body, columns refer to the CTE's output (a WorkTableScan / CteScan),
    // which has no catalog entry. Resolve such names against the CTE scope.
    if let Some(scope) = ctx.cte_scope.borrow().as_ref() {
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
        // Unqualified: search relations for the one owning this column.
        let mut found = None;
        for (t, &rtindex) in &ctx.rtindex_map {
            if let Some(&reloid) = ctx.rtoid_map.get(t) {
                if let Some(r) = resolve(rtindex, reloid) {
                    found = Some(r);
                    break;
                }
            }
        }
        match found {
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
    let left_pg = translate(left, ctx);
    let right_pg = translate(right, ctx);
    if left_pg.is_null() || right_pg.is_null() {
        return std::ptr::null_mut();
    }

    let op_str = match op {
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
        _ => return std::ptr::null_mut(),
    };

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
        return std::ptr::null_mut();
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

    let node = alloc::<pg_sys::FuncExpr>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_FuncExpr;
    (*node).funcid = funcoid;
    (*node).funcresulttype = rettype;
    (*node).funcretset = false;
    (*node).funcvariadic = false;
    (*node).funcformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CALL;
    (*node).funccollid = pg_sys::InvalidOid;
    (*node).inputcollid = pg_sys::InvalidOid;
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

    let node = alloc::<pg_sys::CoerceViaIO>();
    (*node).xpr.type_ = pg_sys::NodeTag::T_CoerceViaIO;
    (*node).arg = arg.cast();
    (*node).resulttype = target_oid;
    (*node).resultcollid = pg_sys::InvalidOid;
    (*node).coerceformat = pg_sys::CoercionForm::COERCE_EXPLICIT_CAST;
    (node as *mut pg_sys::Expr)
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
    match (*expr).type_ {
        pg_sys::NodeTag::T_Const => (*(expr as *mut pg_sys::Const)).consttype,
        pg_sys::NodeTag::T_Var => (*(expr as *mut pg_sys::Var)).vartype,
        pg_sys::NodeTag::T_OpExpr => (*(expr as *mut pg_sys::OpExpr)).opresulttype,
        pg_sys::NodeTag::T_BoolExpr => pg_sys::BOOLOID,
        pg_sys::NodeTag::T_NullTest => pg_sys::BOOLOID,
        pg_sys::NodeTag::T_FuncExpr => (*(expr as *mut pg_sys::FuncExpr)).funcresulttype,
        pg_sys::NodeTag::T_CaseExpr => (*(expr as *mut pg_sys::CaseExpr)).casetype,
        pg_sys::NodeTag::T_CoerceViaIO => (*(expr as *mut pg_sys::CoerceViaIO)).resulttype,
        pg_sys::NodeTag::T_ArrayExpr => (*(expr as *mut pg_sys::ArrayExpr)).array_typeid,
        _ => pg_sys::InvalidOid,
    }
}

/// Allocate a zeroed node in the current PostgreSQL memory context.
unsafe fn alloc<T>() -> *mut T {
    pg_sys::palloc0(std::mem::size_of::<T>()).cast()
}
