//! Direct catalog resolution: map Ra table/column names to PG OIDs
//! without depending on PG's parsed Query tree.
//!
//! This module replaces the former dependency on `Query.rtable` — Ra's
//! Lime parser produces table names, and this module resolves them
//! directly against PostgreSQL's system catalogs via `SearchSysCache`.

use std::collections::HashMap;
use std::ffi::CString;

use pgrx::pg_sys;

/// A resolved table entry — everything needed to build a PG RangeTblEntry.
#[derive(Debug, Clone)]
pub struct ResolvedTable {
    /// Name as it appears in the query (might be alias).
    pub query_name: String,
    /// Actual relation name from catalog.
    pub rel_name: String,
    /// Schema name.
    pub schema_name: String,
    /// Relation OID.
    pub relid: pg_sys::Oid,
    /// Relation kind (r=table, v=view, m=matview, etc.).
    pub relkind: i8,
    /// Position in the range table (1-based).
    pub rtindex: pg_sys::Index,
    /// Optional alias (FROM table AS alias).
    pub alias: Option<String>,
}

/// Resolved catalog information for all tables in a query.
pub struct CatalogResolution {
    /// Table name/alias → resolved entry.
    pub table_map: HashMap<String, (pg_sys::Index, pg_sys::Oid)>,
    /// Ordered list of resolved tables (for rtable construction).
    pub tables: Vec<ResolvedTable>,
}

/// Extract all table names and aliases from a RelExpr tree.
pub fn collect_table_refs(expr: &ra_core::algebra::RelExpr) -> Vec<(String, Option<String>)> {
    let mut refs = Vec::new();
    collect_table_refs_rec(expr, &mut refs);
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    refs.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    refs
}

fn collect_table_refs_rec(
    expr: &ra_core::algebra::RelExpr,
    out: &mut Vec<(String, Option<String>)>,
) {
    match expr {
        ra_core::algebra::RelExpr::Scan { table, alias } => {
            out.push((table.clone(), alias.clone()));
        }
        other => {
            for child in other.children() {
                collect_table_refs_rec(child, out);
            }
        }
    }
}

/// Resolve table names against the PostgreSQL catalog.
///
/// # Safety
///
/// Accesses PostgreSQL system caches (requires valid transaction context).
pub unsafe fn resolve_tables(
    table_refs: &[(String, Option<String>)],
    search_path_ns: pg_sys::Oid,
) -> CatalogResolution {
    let mut table_map = HashMap::new();
    let mut tables = Vec::new();

    for (i, (table_name, alias)) in table_refs.iter().enumerate() {
        let rtindex = (i + 1) as pg_sys::Index;

        // Look up the relation OID via the search path.
        let c_name = match CString::new(table_name.as_str()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let relid = pg_sys::get_relname_relid(c_name.as_ptr(), search_path_ns);
        if relid == pg_sys::InvalidOid {
            // Try public schema as fallback.
            let public_ns = pg_sys::get_namespace_oid(
                c"public".as_ptr(),
                true, // missing_ok
            );
            let relid2 = pg_sys::get_relname_relid(c_name.as_ptr(), public_ns);
            if relid2 == pg_sys::InvalidOid {
                continue;
            }
            let entry = resolve_single_table(table_name, alias, relid2, rtindex);
            table_map.insert(table_name.to_lowercase(), (rtindex, relid2));
            if let Some(a) = alias {
                table_map.entry(a.to_lowercase()).or_insert((rtindex, relid2));
            }
            tables.push(entry);
        } else {
            let entry = resolve_single_table(table_name, alias, relid, rtindex);
            table_map.insert(table_name.to_lowercase(), (rtindex, relid));
            if let Some(a) = alias {
                table_map.entry(a.to_lowercase()).or_insert((rtindex, relid));
            }
            tables.push(entry);
        }
    }

    CatalogResolution { table_map, tables }
}

unsafe fn resolve_single_table(
    table_name: &str,
    alias: &Option<String>,
    relid: pg_sys::Oid,
    rtindex: pg_sys::Index,
) -> ResolvedTable {
    let rel_name = {
        let ptr = pg_sys::get_rel_name(relid);
        if ptr.is_null() {
            table_name.to_string()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };

    let ns_oid = get_rel_namespace(relid);
    let schema_name = {
        let ptr = pg_sys::get_namespace_name(ns_oid);
        if ptr.is_null() {
            "public".to_string()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    };

    let relkind = get_rel_relkind(relid);

    ResolvedTable {
        query_name: alias.as_deref().unwrap_or(table_name).to_string(),
        rel_name,
        schema_name,
        relid,
        relkind,
        rtindex,
        alias: alias.clone(),
    }
}

unsafe fn get_rel_namespace(relid: pg_sys::Oid) -> pg_sys::Oid {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(relid),
    );
    if tuple.is_null() {
        return pg_sys::InvalidOid;
    }
    let class_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let ns = (*class_form).relnamespace;
    pg_sys::ReleaseSysCache(tuple);
    ns
}

unsafe fn get_rel_relkind(relid: pg_sys::Oid) -> i8 {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(relid),
    );
    if tuple.is_null() {
        return b'r' as i8;
    }
    let class_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let kind = (*class_form).relkind;
    pg_sys::ReleaseSysCache(tuple);
    kind
}

/// Build a PostgreSQL rtable (List of RangeTblEntry) from resolved tables.
///
/// # Safety
///
/// Allocates in the current memory context (must be a valid PG context).
pub unsafe fn build_rtable(resolution: &CatalogResolution) -> *mut pg_sys::List {
    let mut rtable: *mut pg_sys::List = std::ptr::null_mut();

    for table in &resolution.tables {
        let rte = pg_sys::palloc0(std::mem::size_of::<pg_sys::RangeTblEntry>())
            as *mut pg_sys::RangeTblEntry;
        (*rte).type_ = pg_sys::NodeTag::T_RangeTblEntry;
        (*rte).rtekind = pg_sys::RTEKind::RTE_RELATION;
        (*rte).relid = table.relid;
        (*rte).relkind = table.relkind as std::ffi::c_char;
        (*rte).rellockmode = pg_sys::AccessShareLock as i32;
        (*rte).lateral = false;
        (*rte).inFromCl = true;
        (*rte).inh = true; // include inheritance children

        // Set the alias/eref
        let alias_str = table.alias.as_deref().unwrap_or(&table.rel_name);
        let c_alias = CString::new(alias_str).unwrap_or_default();
        let eref = pg_sys::palloc0(std::mem::size_of::<pg_sys::Alias>()) as *mut pg_sys::Alias;
        (*eref).type_ = pg_sys::NodeTag::T_Alias;
        (*eref).aliasname = pg_sys::pstrdup(c_alias.as_ptr());
        (*rte).eref = eref;

        if table.alias.is_some() {
            let alias_node =
                pg_sys::palloc0(std::mem::size_of::<pg_sys::Alias>()) as *mut pg_sys::Alias;
            (*alias_node).type_ = pg_sys::NodeTag::T_Alias;
            (*alias_node).aliasname = pg_sys::pstrdup(c_alias.as_ptr());
            (*rte).alias = alias_node;
        }

        rtable = pg_sys::lappend(rtable, rte.cast());
    }

    rtable
}

/// Build RTEPermInfos for the resolved tables (PG17+).
///
/// # Safety
///
/// Allocates in the current memory context.
pub unsafe fn build_perm_infos(resolution: &CatalogResolution) -> *mut pg_sys::List {
    let mut infos: *mut pg_sys::List = std::ptr::null_mut();

    for table in &resolution.tables {
        let info = pg_sys::palloc0(std::mem::size_of::<pg_sys::RTEPermissionInfo>())
            as *mut pg_sys::RTEPermissionInfo;
        (*info).type_ = pg_sys::NodeTag::T_RTEPermissionInfo;
        (*info).relid = table.relid;
        // SELECT permission
        (*info).requiredPerms = pg_sys::ACL_SELECT as pg_sys::AclMode;
        infos = pg_sys::lappend(infos, info.cast());
    }

    infos
}
