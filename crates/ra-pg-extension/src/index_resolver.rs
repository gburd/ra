//! Index OID resolution for the plan builder.
//!
//! Resolves index OIDs from PostgreSQL system catalogs so that
//! `IndexScan`, `BitmapIndexScan`, and `IndexOnlyScan` plan nodes
//! can reference the correct index.

use std::ffi::CStr;

use pgrx::pg_sys;

/// Metadata about a resolved index.
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// The index's OID in pg_class.
    pub oid: pg_sys::Oid,
    /// The relation (table) this index belongs to.
    pub rel_oid: pg_sys::Oid,
    /// Column names covered by the index, in key order.
    pub columns: Vec<String>,
    /// Access method type name ("btree", "hash", "gin", "gist", "brin", "spgist").
    pub am_type: String,
    /// Whether this index enforces uniqueness.
    pub is_unique: bool,
}

/// Resolve an index on `rel_oid` that covers `column_name` as its first key column.
///
/// Iterates the relation's index list and returns the first btree index
/// whose leading column matches `column_name`. Prefers unique indexes.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process with a valid
/// transaction context. The relation must be accessible under the current
/// user's permissions.
pub unsafe fn resolve_index(rel_oid: pg_sys::Oid, column_name: &str) -> Option<IndexInfo> {
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return None;
    }

    let index_list = pg_sys::RelationGetIndexList(rel);
    // An empty index set is NIL (a null List pointer); guard before deref.
    let n_indexes = if index_list.is_null() {
        0
    } else {
        (*index_list).length
    };

    let mut best: Option<IndexInfo> = None;

    for i in 0..n_indexes {
        let idx_oid = pg_sys::list_nth_oid(index_list, i);

        if let Some(info) = read_index_info(idx_oid, rel_oid) {
            // Check if the first column matches
            if let Some(first_col) = info.columns.first() {
                if first_col.eq_ignore_ascii_case(column_name) {
                    // Prefer unique indexes
                    if info.is_unique {
                        pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
                        return Some(info);
                    }
                    if best.is_none() {
                        best = Some(info);
                    }
                }
            }
        }
    }

    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    best
}

/// Resolve an index by its name on the given relation.
///
/// Looks up the named index in the relation's index list by matching
/// against `pg_class.relname`.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn resolve_index_by_name(rel_oid: pg_sys::Oid, index_name: &str) -> Option<IndexInfo> {
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return None;
    }

    let index_list = pg_sys::RelationGetIndexList(rel);
    // An empty index set is NIL (a null List pointer); guard before deref.
    let n_indexes = if index_list.is_null() {
        0
    } else {
        (*index_list).length
    };

    for i in 0..n_indexes {
        let idx_oid = pg_sys::list_nth_oid(index_list, i);

        // Check if this index's name matches
        let class_tuple = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::RELOID as i32,
            pg_sys::Datum::from(idx_oid),
        );
        if class_tuple.is_null() {
            continue;
        }

        let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
        let name = CStr::from_ptr((*class_form).relname.data.as_ptr())
            .to_string_lossy();
        let matches = name.eq_ignore_ascii_case(index_name);
        pg_sys::ReleaseSysCache(class_tuple);

        if matches {
            if let Some(info) = read_index_info(idx_oid, rel_oid) {
                pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
                return Some(info);
            }
        }
    }

    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    None
}

/// Find a btree index whose key columns COVER all `needed` columns (every
/// needed column is one of the index's key columns), so an index-only scan can
/// satisfy the query without a heap fetch.
///
/// Among covering candidates the one with the fewest key columns is preferred
/// (less I/O), with unique indexes winning ties. Returns `None` when no btree
/// index covers every needed column.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process.
pub unsafe fn find_covering_index(rel_oid: pg_sys::Oid, needed: &[String]) -> Option<IndexInfo> {
    if needed.is_empty() {
        return None;
    }
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return None;
    }
    let index_list = pg_sys::RelationGetIndexList(rel);
    let n_indexes = if index_list.is_null() { 0 } else { (*index_list).length };

    let covers = |info: &IndexInfo| -> bool {
        needed
            .iter()
            .all(|n| info.columns.iter().any(|c| c.eq_ignore_ascii_case(n)))
    };

    let mut best: Option<IndexInfo> = None;
    for i in 0..n_indexes {
        let idx_oid = pg_sys::list_nth_oid(index_list, i);
        let Some(info) = read_index_info(idx_oid, rel_oid) else {
            continue;
        };
        if info.am_type != "btree" || !covers(&info) {
            continue;
        }
        let better = match &best {
            None => true,
            Some(b) => {
                info.columns.len() < b.columns.len()
                    || (info.columns.len() == b.columns.len() && info.is_unique && !b.is_unique)
            }
        };
        if better {
            best = Some(info);
        }
    }

    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    best
}

/// Read full index metadata from system caches.
///
/// Returns `None` if the index OID is invalid or the catalog entries
/// are inaccessible.
unsafe fn read_index_info(idx_oid: pg_sys::Oid, rel_oid: pg_sys::Oid) -> Option<IndexInfo> {
    // Look up pg_class for index name and AM
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if class_tuple.is_null() {
        return None;
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
    let am_oid = (*class_form).relam;
    pg_sys::ReleaseSysCache(class_tuple);

    let am_type = resolve_am_name(am_oid);

    // Look up pg_index for column info and uniqueness
    let idx_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::INDEXRELID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if idx_tuple.is_null() {
        return None;
    }

    let idx_form = pg_sys::GETSTRUCT(idx_tuple) as *mut pg_sys::FormData_pg_index;
    let is_unique = (*idx_form).indisunique;
    let natts = (*idx_form).indnatts as usize;

    // Read indexed column names
    let mut columns = Vec::with_capacity(natts);
    for i in 0..natts {
        let attnum = (*idx_form).indkey.values.as_slice(natts)[i];
        if attnum > 0 {
            if let Some(name) = read_attname(rel_oid, attnum) {
                columns.push(name);
            } else {
                columns.push(format!("col_{attnum}"));
            }
        } else {
            // Expression index column
            columns.push(format!("expr_{i}"));
        }
    }

    pg_sys::ReleaseSysCache(idx_tuple);

    Some(IndexInfo {
        oid: idx_oid,
        rel_oid,
        columns,
        am_type,
        is_unique,
    })
}

/// Resolve an access method OID to its name string.
unsafe fn resolve_am_name(am_oid: pg_sys::Oid) -> String {
    let am_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::AMOID as i32,
        pg_sys::Datum::from(am_oid),
    );
    if am_tuple.is_null() {
        return "unknown".to_string();
    }
    let am_form = pg_sys::GETSTRUCT(am_tuple) as *mut pg_sys::FormData_pg_am;
    let name = CStr::from_ptr((*am_form).amname.data.as_ptr())
        .to_string_lossy()
        .into_owned();
    pg_sys::ReleaseSysCache(am_tuple);
    name
}

/// Read an attribute name from pg_attribute by (rel_oid, attnum).
unsafe fn read_attname(rel_oid: pg_sys::Oid, attnum: i16) -> Option<String> {
    let att_tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
    );
    if att_tuple.is_null() {
        return None;
    }
    let att_form = pg_sys::GETSTRUCT(att_tuple) as *mut pg_sys::FormData_pg_attribute;
    if (*att_form).attisdropped {
        pg_sys::ReleaseSysCache(att_tuple);
        return None;
    }
    let name = CStr::from_ptr((*att_form).attname.data.as_ptr())
        .to_string_lossy()
        .into_owned();
    pg_sys::ReleaseSysCache(att_tuple);
    Some(name)
}
