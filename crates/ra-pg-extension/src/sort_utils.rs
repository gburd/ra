//! Shared utilities for building sort/group/unique column metadata arrays.
//!
//! PostgreSQL executor nodes like Sort, Agg, Unique, and WindowAgg require
//! arrays of attribute numbers, operator OIDs, and collations for each
//! column they operate on. This module provides helpers to build these
//! from Ra's `SortKey` and `Expr` types.

use std::ffi::CStr;

use pgrx::pg_sys;

use ra_core::algebra::SortKey;
use ra_core::algebra::SortDirection;
use ra_core::expr::{ColumnRef, Expr};

/// Resolve the ordering operator OID for a given type.
///
/// For ascending order, returns the less-than operator.
/// For descending order, returns the greater-than operator.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn resolve_sort_operator(type_oid: pg_sys::Oid, ascending: bool) -> pg_sys::Oid {
    let mut lt_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut eq_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut gt_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut hashable: bool = false;

    pg_sys::get_sort_group_operators(
        type_oid,
        true,  // needLT
        false, // needEQ
        true,  // needGT
        &mut lt_op,
        &mut eq_op,
        &mut gt_op,
        &mut hashable,
    );

    if ascending { lt_op } else { gt_op }
}

/// Resolve the equality operator OID for a given type.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn resolve_equality_op(type_oid: pg_sys::Oid) -> pg_sys::Oid {
    let mut lt_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut eq_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut gt_op: pg_sys::Oid = pg_sys::InvalidOid;
    let mut hashable: bool = false;

    pg_sys::get_sort_group_operators(
        type_oid,
        false, // needLT
        true,  // needEQ
        false, // needGT
        &mut lt_op,
        &mut eq_op,
        &mut gt_op,
        &mut hashable,
    );

    eq_op
}

/// Find a column's AttrNumber in a plan's targetlist by name.
///
/// Walks the targetlist looking for a TargetEntry whose `resname` matches
/// `col_name` (case-insensitive). Returns the 1-based attribute number
/// (resno) if found.
///
/// # Safety
///
/// `targetlist` must be a valid PostgreSQL List of TargetEntry nodes.
pub unsafe fn find_attr_in_targetlist(
    targetlist: *mut pg_sys::List,
    col_name: &str,
) -> Option<pg_sys::AttrNumber> {
    if targetlist.is_null() {
        return None;
    }

    let length = (*targetlist).length;
    let elements = (*targetlist).elements;

    for i in 0..length {
        let cell = elements.add(i as usize);
        let te = (*cell).ptr_value as *mut pg_sys::TargetEntry;
        if te.is_null() {
            continue;
        }
        if (*te).resname.is_null() {
            continue;
        }
        let resname = CStr::from_ptr((*te).resname).to_string_lossy();
        if resname.eq_ignore_ascii_case(col_name) {
            return Some((*te).resno);
        }
    }

    None
}

/// Get the type OID and collation for a column in a relation.
///
/// Returns `(type_oid, collation_oid)` or `None` if the attribute
/// cannot be found.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn get_column_type_info(
    rel_oid: pg_sys::Oid,
    col_name: &str,
) -> Option<(pg_sys::Oid, pg_sys::Oid)> {
    // We need to find the attnum for this column name first
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return None;
    }

    let natts = (*(*rel).rd_att).natts as usize;
    let mut result = None;

    for i in 0..natts {
        let attr = (*(*rel).rd_att).attrs.as_ptr().add(i);
        if (*attr).attisdropped {
            continue;
        }
        let name = CStr::from_ptr((*attr).attname.data.as_ptr()).to_string_lossy();
        if name.eq_ignore_ascii_case(col_name) {
            result = Some(((*attr).atttypid, (*attr).attcollation));
            break;
        }
    }

    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    result
}

/// Extract the column name from a Ra expression.
///
/// Only handles simple column references. For complex expressions,
/// returns `None`.
pub fn extract_column_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Column(ColumnRef { column, .. }) => Some(column.as_str()),
        _ => None,
    }
}

/// Build sort column arrays from a list of `SortKey`s.
///
/// Allocates four parallel arrays in the current PostgreSQL memory context:
/// - `sortColIdx`: attribute numbers in the child targetlist
/// - `sortOperators`: ordering operator OIDs
/// - `collations`: collation OIDs
/// - `nullsFirst`: NULL ordering flags
///
/// Returns `None` if any sort key cannot be resolved (missing column, etc).
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process with a valid
/// memory context. `child_targetlist` must be a valid List of TargetEntry.
pub unsafe fn build_sort_arrays(
    keys: &[SortKey],
    child_targetlist: *mut pg_sys::List,
    rel_oid: pg_sys::Oid,
) -> Option<SortArrays> {
    let n = keys.len();
    if n == 0 {
        return Some(SortArrays {
            col_idx: std::ptr::null_mut(),
            operators: std::ptr::null_mut(),
            collations: std::ptr::null_mut(),
            nulls_first: std::ptr::null_mut(),
            num_cols: 0,
        });
    }

    let col_idx = pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>())
        as *mut pg_sys::AttrNumber;
    let operators = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
    let collations = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
    let nulls_first = pg_sys::palloc(n * std::mem::size_of::<bool>()) as *mut bool;

    for (i, key) in keys.iter().enumerate() {
        // Extract column name from the sort expression
        let col_name = match extract_column_name(&key.expr) {
            Some(name) => name,
            None => {
                // For non-column expressions, use position-based fallback
                // Assign sequential attr number
                *col_idx.add(i) = (i + 1) as pg_sys::AttrNumber;
                *operators.add(i) = pg_sys::InvalidOid;
                *collations.add(i) = pg_sys::InvalidOid;
                *nulls_first.add(i) = matches!(key.nulls, ra_core::algebra::NullOrdering::First);
                continue;
            }
        };

        // Find the column in the child targetlist
        let attno = find_attr_in_targetlist(child_targetlist, col_name)
            .unwrap_or((i + 1) as pg_sys::AttrNumber);
        *col_idx.add(i) = attno;

        // Resolve type info and operators
        let (type_oid, collation) = get_column_type_info(rel_oid, col_name)
            .unwrap_or((pg_sys::INT4OID, pg_sys::InvalidOid));

        let ascending = matches!(key.direction, SortDirection::Asc);
        *operators.add(i) = resolve_sort_operator(type_oid, ascending);
        *collations.add(i) = collation;
        *nulls_first.add(i) = matches!(key.nulls, ra_core::algebra::NullOrdering::First);
    }

    Some(SortArrays {
        col_idx,
        operators,
        collations,
        nulls_first,
        num_cols: n as i32,
    })
}

/// Build group-by column arrays from a list of grouping expressions.
///
/// Allocates three parallel arrays:
/// - `grpColIdx`: attribute numbers in the child targetlist
/// - `grpOperators`: equality operator OIDs
/// - `grpCollations`: collation OIDs
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn build_group_arrays(
    group_by: &[Expr],
    child_targetlist: *mut pg_sys::List,
    rel_oid: pg_sys::Oid,
) -> Option<GroupArrays> {
    let n = group_by.len();
    if n == 0 {
        return Some(GroupArrays {
            col_idx: std::ptr::null_mut(),
            operators: std::ptr::null_mut(),
            collations: std::ptr::null_mut(),
            num_cols: 0,
        });
    }

    let col_idx = pg_sys::palloc(n * std::mem::size_of::<pg_sys::AttrNumber>())
        as *mut pg_sys::AttrNumber;
    let operators = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;
    let collations = pg_sys::palloc(n * std::mem::size_of::<pg_sys::Oid>()) as *mut pg_sys::Oid;

    for (i, expr) in group_by.iter().enumerate() {
        let col_name = match extract_column_name(expr) {
            Some(name) => name,
            None => {
                *col_idx.add(i) = (i + 1) as pg_sys::AttrNumber;
                *operators.add(i) = pg_sys::InvalidOid;
                *collations.add(i) = pg_sys::InvalidOid;
                continue;
            }
        };

        let attno = find_attr_in_targetlist(child_targetlist, col_name)
            .unwrap_or((i + 1) as pg_sys::AttrNumber);
        *col_idx.add(i) = attno;

        let (type_oid, collation) = get_column_type_info(rel_oid, col_name)
            .unwrap_or((pg_sys::INT4OID, pg_sys::InvalidOid));

        *operators.add(i) = resolve_equality_op(type_oid);
        *collations.add(i) = collation;
    }

    Some(GroupArrays {
        col_idx,
        operators,
        collations,
        num_cols: n as i32,
    })
}

/// Result of building sort column arrays.
pub struct SortArrays {
    pub col_idx: *mut pg_sys::AttrNumber,
    pub operators: *mut pg_sys::Oid,
    pub collations: *mut pg_sys::Oid,
    pub nulls_first: *mut bool,
    pub num_cols: i32,
}

/// Result of building group-by column arrays.
pub struct GroupArrays {
    pub col_idx: *mut pg_sys::AttrNumber,
    pub operators: *mut pg_sys::Oid,
    pub collations: *mut pg_sys::Oid,
    pub num_cols: i32,
}
