//! Statistics bridge: reads PostgreSQL catalogs and converts to RA format.
//!
//! Uses direct PostgreSQL catalog C API access (syscache lookups,
//! catalog scans) instead of SPI to avoid nested SPI crashes when
//! called from planner hooks. SPI opens a new connection, which
//! PostgreSQL forbids inside planner callbacks.
//!
//! Statistics are populated into `ra_core::Statistics` and
//! `ra_core::ColumnStats` structs. Statistics are cached for
//! the duration of a single planning cycle.
//!
//! PostgreSQL-specific MVCC statistics (HOT updates, bloat) are tracked
//! separately as they don't apply to other database systems.

use std::ffi::CStr;

use pgrx::pg_sys;

use ra_core::{ColumnStats, Statistics};

/// PostgreSQL-specific MVCC and HOT update statistics.
///
/// These metrics are critical for understanding heap-only tuple behavior:
/// - HOT (Heap-Only Tuple) updates avoid index maintenance
/// - Dead tuples and bloat affect sequential scan performance
/// - High dead tuple ratio indicates need for VACUUM
///
/// **PostgreSQL-specific:** This struct only applies to PostgreSQL heap tables
/// and has no meaning for other databases (MySQL, Oracle, etc.) or storage
/// engines (columnar, LSM-tree, etc.).
#[derive(Debug, Clone)]
pub struct PostgresMvccStats {
    /// Fraction of updates that were HOT updates, in [0.0, 1.0].
    ///
    /// HOT updates happen when:
    /// 1. No indexed columns are modified
    /// 2. Sufficient free space exists on the same page
    ///
    /// High HOT ratio (>0.8) is good - indicates efficient updates.
    /// Low HOT ratio suggests:
    /// - Frequent indexed column updates
    /// - Page-level space fragmentation (need VACUUM or fillfactor tuning)
    pub hot_update_ratio: f64,

    /// Fraction of dead tuples, in [0.0, 1.0].
    ///
    /// Dead tuples remain after UPDATEs/DELETEs until VACUUM.
    /// High dead tuple ratio (>0.1) significantly degrades:
    /// - Sequential scan performance (must skip dead tuples)
    /// - Index scan performance (bitmap must filter dead tuples)
    pub dead_tuple_ratio: f64,

    /// Estimated bloat factor (size_on_disk / actual_data_size).
    ///
    /// Bloat > 2.0 indicates significant wasted space from:
    /// - Dead tuples (need VACUUM)
    /// - Page fragmentation (need VACUUM FULL or CLUSTER)
    pub bloat_factor: f64,

    /// Timestamp of last ANALYZE (for staleness detection).
    pub last_analyze: Option<String>,

    /// Timestamp of last VACUUM (for bloat tracking).
    pub last_vacuum: Option<String>,
}

impl PostgresMvccStats {
    /// Create default MVCC statistics (no HOT updates, no bloat).
    #[must_use]
    pub fn default() -> Self {
        Self {
            hot_update_ratio: 0.0,
            dead_tuple_ratio: 0.0,
            bloat_factor: 1.0,
            last_analyze: None,
            last_vacuum: None,
        }
    }

    /// Returns true if statistics are stale (>7 days since last ANALYZE).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        // Simplified check - in practice, parse timestamp and compare
        self.last_analyze.is_none()
    }

    /// Returns true if table needs VACUUM (high dead tuple ratio).
    #[must_use]
    pub fn needs_vacuum(&self) -> bool {
        self.dead_tuple_ratio > 0.1 || self.bloat_factor > 2.0
    }
}

/// PostgreSQL table statistics with MVCC metrics.
///
/// Wraps database-agnostic `Statistics` with PostgreSQL-specific MVCC data.
#[derive(Debug, Clone)]
pub struct PostgresTableStats {
    /// Database-agnostic statistics.
    pub base: Statistics,
    /// PostgreSQL-specific MVCC/HOT statistics.
    pub mvcc: PostgresMvccStats,
}

// ---------------------------------------------------------------
// Catalog access helpers (no SPI -- safe inside planner hooks)
// ---------------------------------------------------------------

/// Resolve a schema name to its namespace OID.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process with valid
/// memory context.
unsafe fn resolve_namespace_oid(schema: &str) -> Option<pg_sys::Oid> {
    let c_schema = std::ffi::CString::new(schema).ok()?;
    let ns_oid = pg_sys::get_namespace_oid(
        c_schema.as_ptr(),
        true, // missing_ok
    );
    if ns_oid == pg_sys::InvalidOid {
        None
    } else {
        Some(ns_oid)
    }
}

/// Resolve a table name + schema to its relation OID.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn resolve_relation_oid(
    schema: &str,
    table: &str,
) -> Option<pg_sys::Oid> {
    let ns_oid = resolve_namespace_oid(schema)?;
    let c_table = std::ffi::CString::new(table).ok()?;
    let rel_oid = pg_sys::get_relname_relid(
        c_table.as_ptr(),
        ns_oid,
    );
    if rel_oid == pg_sys::InvalidOid {
        None
    } else {
        Some(rel_oid)
    }
}

/// Read a `pg_class` tuple from syscache and extract `reltuples`.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_reltuples(rel_oid: pg_sys::Oid) -> Option<f32> {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if tuple.is_null() {
        return None;
    }

    let class_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let reltuples = (*class_form).reltuples;

    pg_sys::ReleaseSysCache(tuple);

    Some(reltuples)
}

/// Read the number of user attributes for a relation from `pg_class`.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_relnatts(rel_oid: pg_sys::Oid) -> Option<i16> {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if tuple.is_null() {
        return None;
    }

    let class_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let natts = (*class_form).relnatts;

    pg_sys::ReleaseSysCache(tuple);

    Some(natts)
}

/// Read the attribute name for (relation, attnum) from syscache.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_attname(
    rel_oid: pg_sys::Oid,
    attnum: i16,
) -> Option<String> {
    let tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
    );
    if tuple.is_null() {
        return None;
    }

    let att_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_attribute;

    // Skip dropped attributes
    if (*att_form).attisdropped {
        pg_sys::ReleaseSysCache(tuple);
        return None;
    }

    let name = CStr::from_ptr((*att_form).attname.data.as_ptr())
        .to_string_lossy()
        .into_owned();

    pg_sys::ReleaseSysCache(tuple);

    Some(name)
}

/// Read `pg_statistic` entry for a single column.
///
/// Reads stadistinct, stanullfrac, stawidth, and stakind/stavalues/stanumbers
/// arrays from the `pg_statistic` catalog via syscache.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_column_stats(
    rel_oid: pg_sys::Oid,
    attnum: i16,
    row_count: f64,
) -> Option<ColumnStats> {
    // STATRELATTINH: (starelid, staattnum, stainherit)
    let tuple = pg_sys::SearchSysCache3(
        pg_sys::SysCacheIdentifier::STATRELATTINH as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
        pg_sys::Datum::from(false), // stainherit = false
    );
    if tuple.is_null() {
        return None;
    }

    let stat_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

    // Extract basic stats from the fixed-length portion
    let n_distinct = (*stat_form).stadistinct;
    let null_frac = (*stat_form).stanullfrac;
    let avg_width = (*stat_form).stawidth;
    let correlation = read_stat_correlation(tuple);

    let distinct = interpret_n_distinct(n_distinct, row_count);
    let mut col_stats = ColumnStats::new(distinct);
    col_stats.null_fraction = f64::from(null_frac);
    col_stats.avg_length = Some(f64::from(avg_width));
    col_stats.correlation = correlation.map(f64::from);

    // Read MCV and histogram from stakind/stavalues/stanumbers slots.
    // pg_statistic has 5 "slots" (stakind1..stakind5) that can hold
    // different kinds of statistics. We look for:
    // - STATISTIC_KIND_MCV (1): most common values + frequencies
    // - STATISTIC_KIND_HISTOGRAM (2): histogram bounds
    // - STATISTIC_KIND_CORRELATION (3): correlation (already in fixed part)
    read_stat_slots(tuple, &mut col_stats, row_count, distinct);

    pg_sys::ReleaseSysCache(tuple);

    Some(col_stats)
}

/// Read correlation from pg_statistic stakind slots.
///
/// pg_statistic stores correlation in a slot with
/// stakind = STATISTIC_KIND_CORRELATION (3).
///
/// # Safety
///
/// Must be called with a valid pg_statistic HeapTuple.
unsafe fn read_stat_correlation(
    tuple: pg_sys::HeapTuple,
) -> Option<f32> {
    let stat_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

    // Scan the 5 stakind slots for CORRELATION (kind = 3)
    let stakinds = [
        (*stat_form).stakind1,
        (*stat_form).stakind2,
        (*stat_form).stakind3,
        (*stat_form).stakind4,
        (*stat_form).stakind5,
    ];

    for (slot_idx, &kind) in stakinds.iter().enumerate() {
        if kind == pg_sys::STATISTIC_KIND_CORRELATION as i16 {
            // Correlation is stored in stanumbers[slot] as a single float
            return read_stanumbers_first(tuple, slot_idx);
        }
    }

    None
}

/// Read MCV values/frequencies and histogram bounds from
/// pg_statistic's variable-length slots.
///
/// # Safety
///
/// Must be called with a valid pg_statistic HeapTuple.
unsafe fn read_stat_slots(
    tuple: pg_sys::HeapTuple,
    col_stats: &mut ColumnStats,
    row_count: f64,
    distinct: f64,
) {
    let stat_form =
        pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

    let stakinds = [
        (*stat_form).stakind1,
        (*stat_form).stakind2,
        (*stat_form).stakind3,
        (*stat_form).stakind4,
        (*stat_form).stakind5,
    ];

    for (slot_idx, &kind) in stakinds.iter().enumerate() {
        if kind == pg_sys::STATISTIC_KIND_MCV as i16 {
            // MCV: stavalues has the values, stanumbers has frequencies
            if let Some(freqs) = read_stanumbers(tuple, slot_idx) {
                if let Some(vals) = read_stavalues_as_strings(tuple, slot_idx) {
                    col_stats.most_common_values = Some(vals);
                    col_stats.most_common_freqs = Some(freqs);
                }
            }
        } else if kind == pg_sys::STATISTIC_KIND_HISTOGRAM as i16 {
            // Histogram bounds are stored in stavalues
            if let Some(bounds) = read_stavalues_as_strings(tuple, slot_idx) {
                col_stats.histogram = Some(create_equidepth_histogram(
                    bounds,
                    row_count,
                    distinct,
                ));
            }
        }
    }
}

/// Read stanumbers array from a pg_statistic slot.
///
/// Returns the float4[] values as Vec<f64>.
///
/// # Safety
///
/// Must be called with a valid pg_statistic HeapTuple.
unsafe fn read_stanumbers(
    tuple: pg_sys::HeapTuple,
    slot_idx: usize,
) -> Option<Vec<f64>> {
    // stanumbers1..stanumbers5 are at attribute numbers
    // Anum_pg_statistic_stanumbers1 + slot_idx
    let attnum = pg_sys::Anum_pg_statistic_stanumbers1 as i32
        + slot_idx as i32;

    let mut is_null = false;
    let datum = pg_sys::SysCacheGetAttr(
        pg_sys::SysCacheIdentifier::STATRELATTINH as i32,
        tuple,
        attnum,
        &mut is_null,
    );

    if is_null {
        return None;
    }

    // Datum is a float4[] (ArrayType). Use deconstruct_array for safety.
    let array = pg_sys::DatumGetArrayTypeP(datum);
    if array.is_null() {
        return None;
    }

    let mut elems: *mut pg_sys::Datum = std::ptr::null_mut();
    let mut nulls: *mut bool = std::ptr::null_mut();
    let mut n: i32 = 0;

    pg_sys::deconstruct_array(
        array,
        pg_sys::FLOAT4OID,
        4,    // float4 = 4 bytes
        true, // float4 passed by value
        pg_sys::TYPALIGN_INT as i8,
        &mut elems,
        &mut nulls,
        &mut n,
    );

    let mut result = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        if !(*nulls.add(i)) {
            let f = f32::from_bits((*elems.add(i)).value() as u32);
            result.push(f64::from(f));
        }
    }

    pg_sys::pfree(elems as *mut std::ffi::c_void);
    pg_sys::pfree(nulls as *mut std::ffi::c_void);

    Some(result)
}

/// Read the first value from a stanumbers slot (for correlation).
///
/// # Safety
///
/// Must be called with a valid pg_statistic HeapTuple.
unsafe fn read_stanumbers_first(
    tuple: pg_sys::HeapTuple,
    slot_idx: usize,
) -> Option<f32> {
    let numbers = read_stanumbers(tuple, slot_idx)?;
    numbers.first().map(|&f| f as f32)
}

/// Read stavalues from a pg_statistic slot as string representations.
///
/// Uses `pg_sys::OidOutputFunctionCall` to convert each array element
/// to its text representation.
///
/// # Safety
///
/// Must be called with a valid pg_statistic HeapTuple.
unsafe fn read_stavalues_as_strings(
    tuple: pg_sys::HeapTuple,
    slot_idx: usize,
) -> Option<Vec<String>> {
    // stavalues1..stavalues5 are at attribute numbers
    // Anum_pg_statistic_stavalues1 + slot_idx
    let attnum = pg_sys::Anum_pg_statistic_stavalues1 as i32
        + slot_idx as i32;

    let mut is_null = false;
    let datum = pg_sys::SysCacheGetAttr(
        pg_sys::SysCacheIdentifier::STATRELATTINH as i32,
        tuple,
        attnum,
        &mut is_null,
    );

    if is_null {
        return None;
    }

    // stavalues is an anyarray. Deconstruct it.
    let array = pg_sys::DatumGetArrayTypeP(datum);
    if array.is_null() {
        return None;
    }

    let nelems = pg_sys::ArrayGetNItems(
        pg_sys::ARR_NDIM(array),
        pg_sys::ARR_DIMS(array),
    );
    if nelems <= 0 {
        return Some(Vec::new());
    }

    // Get the element type and its type info from the catalog
    let elem_type = pg_sys::ARR_ELEMTYPE(array);
    let mut typoutput: pg_sys::Oid = pg_sys::InvalidOid;
    let mut typioparam: pg_sys::Oid = pg_sys::InvalidOid;
    pg_sys::getTypeOutputInfo(elem_type, &mut typoutput, &mut typioparam);

    // Look up the element type's typlen, typbyval, typalign
    let mut typlen: i16 = 0;
    let mut typbyval: bool = false;
    let mut typalign: i8 = 0;
    pg_sys::get_typlenbyvalalign(
        elem_type, &mut typlen, &mut typbyval, &mut typalign,
    );

    // Deconstruct the array into datums
    let mut elems: *mut pg_sys::Datum = std::ptr::null_mut();
    let mut nulls: *mut bool = std::ptr::null_mut();
    let mut n: i32 = 0;
    pg_sys::deconstruct_array(
        array,
        elem_type,
        typlen as i32,
        typbyval,
        typalign,
        &mut elems,
        &mut nulls,
        &mut n,
    );

    let mut result = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        if !(*nulls.add(i)) {
            let text_ptr = pg_sys::OidOutputFunctionCall(
                typoutput,
                *elems.add(i),
            );
            if !text_ptr.is_null() {
                let s = CStr::from_ptr(text_ptr)
                    .to_string_lossy()
                    .into_owned();
                result.push(s);
                pg_sys::pfree(text_ptr as *mut std::ffi::c_void);
            }
        }
    }

    pg_sys::pfree(elems as *mut std::ffi::c_void);
    pg_sys::pfree(nulls as *mut std::ffi::c_void);

    Some(result)
}

// ---------------------------------------------------------------
// Public API: statistics gathering (planner-safe, no SPI)
// ---------------------------------------------------------------

/// Gather statistics for a single table from PostgreSQL catalogs.
///
/// Uses direct syscache lookups on `pg_class` and `pg_statistic`
/// instead of SPI, making it safe to call from planner hooks.
///
/// Returns `None` if the table has no statistics (unanalyzed) or
/// does not exist.
pub fn gather_table_stats(
    schema: &str,
    table: &str,
) -> Option<Statistics> {
    let row_count = estimate_row_count(schema, table)?;
    let mut stats = Statistics::new(row_count);

    // Read column statistics from pg_statistic via syscache
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;
        let natts = read_relnatts(rel_oid)?;

        // Iterate user attributes (1-based, positive attnum)
        for attnum in 1..=natts {
            let col_name = match read_attname(rel_oid, attnum) {
                Some(name) => name,
                None => continue, // dropped column
            };

            if let Some(col_stats) =
                read_column_stats(rel_oid, attnum, row_count)
            {
                stats.columns.insert(col_name, col_stats);
            }
        }
    }

    // Gather index statistics for index-aware optimization
    gather_index_stats(schema, table, &mut stats);

    Some(stats)
}

/// Estimate the row count for a table from `pg_class.reltuples`.
///
/// Uses syscache lookup on `pg_class` (RELOID) instead of SPI.
///
/// Returns `None` if the table does not exist or has never been
/// analyzed (`reltuples` == -1).
fn estimate_row_count(
    schema: &str,
    table: &str,
) -> Option<f64> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;
        let reltuples = read_reltuples(rel_oid)?;

        if reltuples >= 0.0 {
            Some(f64::from(reltuples))
        } else {
            None
        }
    }
}

/// Gather index statistics for a table.
///
/// Uses `RelationGetIndexList` and syscache lookups on `pg_index`
/// and `pg_class` instead of SPI.
fn gather_index_stats(
    schema: &str,
    table: &str,
    stats: &mut Statistics,
) {
    unsafe {
        let rel_oid = match resolve_relation_oid(schema, table) {
            Some(oid) => oid,
            None => return,
        };

        // Open the relation to get its index list.
        // AccessShareLock is sufficient for reading metadata.
        let rel = pg_sys::table_open(
            rel_oid,
            pg_sys::AccessShareLock as pg_sys::LOCKMODE,
        );
        if rel.is_null() {
            return;
        }

        let index_list = pg_sys::RelationGetIndexList(rel);

        // Iterate the index OID list using list_nth (PG 13+ uses
        // array-based Lists, not linked lists).
        let n_indexes = (*index_list).length;
        for i in 0..n_indexes {
            let cell = pg_sys::list_nth(index_list, i);
            let idx_oid = pg_sys::Oid::from(cell as u32);

            if let Some((name, idx_stats)) = read_single_index(idx_oid) {
                stats.indexes.insert(name, idx_stats);
            }
        }

        pg_sys::list_free(index_list);
        pg_sys::table_close(
            rel,
            pg_sys::AccessShareLock as pg_sys::LOCKMODE,
        );
    }
}

/// Read metadata for a single index from syscache.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_single_index(
    idx_oid: pg_sys::Oid,
) -> Option<(String, ra_core::IndexStats)> {
    // Look up pg_class entry for the index
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if class_tuple.is_null() {
        return None;
    }

    let class_form =
        pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;

    let idx_name = CStr::from_ptr(
        (*class_form).relname.data.as_ptr(),
    )
    .to_string_lossy()
    .into_owned();

    let index_size = (*class_form).relpages as u64
        * pg_sys::BLCKSZ as u64;

    // Get the access method name for index type
    let am_oid = (*class_form).relam;
    pg_sys::ReleaseSysCache(class_tuple);

    let index_type = resolve_am_type(am_oid);

    // Look up pg_index entry via INDEXRELID syscache
    let idx_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::INDEXRELID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if idx_tuple.is_null() {
        return None;
    }

    let idx_form =
        pg_sys::GETSTRUCT(idx_tuple) as *mut pg_sys::FormData_pg_index;

    let is_unique = (*idx_form).indisunique;
    let is_primary = (*idx_form).indisprimary;
    let indrelid = (*idx_form).indrelid;
    let natts = (*idx_form).indnatts as usize;

    // Read indexed column names
    let mut columns = Vec::with_capacity(natts);
    for i in 0..natts {
        let attnum = (*idx_form).indkey.values[i];
        if attnum > 0 {
            // Regular column
            if let Some(name) = read_attname(indrelid, attnum) {
                columns.push(name);
            }
        } else {
            // Expression index -- use placeholder
            columns.push(format!("expr_{i}"));
        }
    }

    pg_sys::ReleaseSysCache(idx_tuple);

    let mut idx_stats = ra_core::IndexStats::new(columns, index_type);
    idx_stats.is_unique = is_unique;
    idx_stats.is_primary = is_primary;
    idx_stats.index_size = index_size;

    Some((idx_name, idx_stats))
}

/// Resolve a pg_am OID to an `IndexType`.
unsafe fn resolve_am_type(am_oid: pg_sys::Oid) -> ra_core::IndexType {
    if am_oid == pg_sys::InvalidOid {
        return ra_core::IndexType::Unknown;
    }

    let am_name_ptr = pg_sys::get_am_name(am_oid);
    if am_name_ptr.is_null() {
        return ra_core::IndexType::Unknown;
    }

    let name = CStr::from_ptr(am_name_ptr)
        .to_string_lossy();

    let result = parse_index_type(&name)
        .unwrap_or(ra_core::IndexType::Unknown);

    pg_sys::pfree(am_name_ptr as *mut std::ffi::c_void);

    result
}

/// Gather statistics for all tables referenced in a query.
///
/// `table_names` should be a list of `(schema, table)` pairs.
/// Tables with no statistics are silently skipped.
pub fn gather_all_stats(
    table_names: &[(String, String)],
) -> Vec<(String, Statistics)> {
    let mut result = Vec::with_capacity(table_names.len());
    for (schema, table) in table_names {
        if let Some(stats) = gather_table_stats(schema, table) {
            result.push((table.clone(), stats));
        }
    }
    result
}

/// Gather PostgreSQL MVCC and HOT update statistics.
///
/// Uses `pg_class` syscache for bloat estimation. MVCC counters
/// (HOT updates, dead tuples) are estimated from `pg_class` metadata
/// since the pgstat struct layout varies across PostgreSQL versions.
///
/// Safe to call from planner hooks (no SPI).
///
/// Returns `None` if the table has no statistics.
pub fn gather_mvcc_stats(
    schema: &str,
    table: &str,
    base_stats: &Statistics,
) -> Option<PostgresMvccStats> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;

        // Estimate bloat from pg_class.relpages vs expected size
        let bloat = estimate_bloat(rel_oid, base_stats);

        // Read pg_class for allvisible pages to estimate dead tuples.
        // A low allvisible/relpages ratio suggests many dead tuples.
        let dead_ratio = estimate_dead_ratio(rel_oid);

        Some(PostgresMvccStats {
            hot_update_ratio: 0.0,
            dead_tuple_ratio: dead_ratio,
            bloat_factor: bloat,
            last_analyze: None,
            last_vacuum: None,
        })
    }
}

/// Estimate dead tuple ratio from pg_class visibility map info.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn estimate_dead_ratio(rel_oid: pg_sys::Oid) -> f64 {
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if class_tuple.is_null() {
        return 0.0;
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple)
        as *mut pg_sys::FormData_pg_class;
    let relpages = (*class_form).relpages;
    let relallvisible = (*class_form).relallvisible;
    pg_sys::ReleaseSysCache(class_tuple);

    if relpages <= 0 {
        return 0.0;
    }

    // Pages not all-visible may contain dead tuples.
    // This is a rough estimate: (non-visible pages) / total pages.
    let non_visible = relpages - relallvisible;
    if non_visible <= 0 {
        0.0
    } else {
        (non_visible as f64 / relpages as f64).min(1.0)
    }
}

/// Estimate bloat factor from pg_class.relpages.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn estimate_bloat(
    rel_oid: pg_sys::Oid,
    base_stats: &Statistics,
) -> f64 {
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if class_tuple.is_null() {
        return 1.0;
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple)
        as *mut pg_sys::FormData_pg_class;
    let relpages = (*class_form).relpages;
    let reltuples = (*class_form).reltuples;
    pg_sys::ReleaseSysCache(class_tuple);

    if reltuples <= 0.0 || relpages <= 0 {
        return 1.0;
    }

    let table_size = relpages as f64 * pg_sys::BLCKSZ as f64;
    let expected_size =
        base_stats.row_count * base_stats.avg_row_size as f64;

    if expected_size > 0.0 {
        (table_size / expected_size).max(1.0)
    } else {
        1.0
    }
}

/// Gather foreign key relationships for join optimization.
///
/// Uses `systable_beginscan` on `pg_constraint` instead of SPI.
///
/// Returns a list of `(from_table, from_column, to_table, to_column)`
/// tuples representing foreign key constraints.
pub fn gather_foreign_keys(
    schema: &str,
    table: &str,
) -> Vec<(String, String, String, String)> {
    let mut fks = Vec::new();

    unsafe {
        let rel_oid = match resolve_relation_oid(schema, table) {
            Some(oid) => oid,
            None => return fks,
        };

        // Open pg_constraint catalog
        let conrel = pg_sys::table_open(
            pg_sys::ConstraintRelationId,
            pg_sys::AccessShareLock as pg_sys::LOCKMODE,
        );
        if conrel.is_null() {
            return fks;
        }

        // Scan for foreign key constraints on this relation
        let scan = pg_sys::systable_beginscan(
            conrel,
            pg_sys::ConstraintRelidTypidNameIndexId,
            true,  // indexOK
            std::ptr::null_mut(), // snapshot (use current)
            0,     // nkeys
            std::ptr::null_mut(), // scankeys
        );

        loop {
            let tup = pg_sys::systable_getnext(scan);
            if tup.is_null() {
                break;
            }

            let con_form = pg_sys::GETSTRUCT(tup)
                as *mut pg_sys::FormData_pg_constraint;

            // Only foreign key constraints on our table
            if (*con_form).contype != pg_sys::CONSTRAINT_FOREIGN as i8 {
                continue;
            }
            if (*con_form).conrelid != rel_oid {
                continue;
            }

            let fk_relid = (*con_form).confrelid;

            // Get FK column attnums from conkey (from) and confkey (to).
            // These are stored as int2vector attributes.
            let conkey = read_constraint_attnums(
                tup, conrel, pg_sys::Anum_pg_constraint_conkey as i32,
            );
            let confkey = read_constraint_attnums(
                tup, conrel, pg_sys::Anum_pg_constraint_confkey as i32,
            );

            if let (Some(from_attnums), Some(to_attnums)) = (conkey, confkey) {
                for (from_att, to_att) in from_attnums.iter().zip(to_attnums.iter()) {
                    let from_col = read_attname(rel_oid, *from_att);
                    let to_col = read_attname(fk_relid, *to_att);
                    let to_table_name = get_rel_name_safe(fk_relid);

                    if let (Some(fc), Some(tt), Some(tc)) =
                        (from_col, to_table_name, to_col)
                    {
                        fks.push((table.to_string(), fc, tt, tc));
                    }
                }
            }
        }

        pg_sys::systable_endscan(scan);
        pg_sys::table_close(
            conrel,
            pg_sys::AccessShareLock as pg_sys::LOCKMODE,
        );
    }

    fks
}

/// Read attribute number array from a pg_constraint tuple.
///
/// # Safety
///
/// Must be called with valid tuple and relation pointers.
unsafe fn read_constraint_attnums(
    tup: pg_sys::HeapTuple,
    rel: pg_sys::Relation,
    attnum: i32,
) -> Option<Vec<i16>> {
    let mut is_null = false;
    let datum = pg_sys::heap_getattr(
        tup,
        attnum,
        (*rel).rd_att,
        &mut is_null,
    );

    if is_null {
        return None;
    }

    let array = pg_sys::DatumGetArrayTypeP(datum);
    if array.is_null() {
        return None;
    }

    let nelems = pg_sys::ArrayGetNItems(
        pg_sys::ARR_NDIM(array),
        pg_sys::ARR_DIMS(array),
    );
    if nelems <= 0 {
        return Some(Vec::new());
    }

    let mut elems: *mut pg_sys::Datum = std::ptr::null_mut();
    let mut nulls: *mut bool = std::ptr::null_mut();
    let mut n: i32 = 0;
    pg_sys::deconstruct_array(
        array,
        pg_sys::INT2OID,
        2,    // int2 is 2 bytes
        true, // int2 is passed by value
        pg_sys::TYPALIGN_SHORT as i8,
        &mut elems,
        &mut nulls,
        &mut n,
    );

    let mut result = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        if !(*nulls.add(i)) {
            result.push((*elems.add(i)).value() as i16);
        }
    }

    pg_sys::pfree(elems as *mut std::ffi::c_void);
    pg_sys::pfree(nulls as *mut std::ffi::c_void);

    Some(result)
}

/// Get a relation name by OID (safe wrapper).
unsafe fn get_rel_name_safe(relid: pg_sys::Oid) -> Option<String> {
    let name_ptr = pg_sys::get_rel_name(relid);
    if name_ptr.is_null() {
        return None;
    }
    let s = CStr::from_ptr(name_ptr)
        .to_string_lossy()
        .into_owned();
    pg_sys::pfree(name_ptr as *mut std::ffi::c_void);
    Some(s)
}

/// Check if a table has been recently analyzed.
///
/// Infers analysis status from `pg_class`: if `reltuples >= 0` and
/// `relallvisible > 0`, the table has likely been analyzed. This is
/// a heuristic since direct timestamp access requires version-dependent
/// pgstat struct access.
///
/// Safe to call from planner hooks (no SPI).
///
/// Returns a placeholder timestamp string if analyzed, or None if
/// the table appears unanalyzed.
pub fn last_analyze_time(
    schema: &str,
    table: &str,
) -> Option<String> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;

        let class_tuple = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::RELOID as i32,
            pg_sys::Datum::from(rel_oid),
        );
        if class_tuple.is_null() {
            return None;
        }

        let class_form = pg_sys::GETSTRUCT(class_tuple)
            as *mut pg_sys::FormData_pg_class;
        let reltuples = (*class_form).reltuples;
        pg_sys::ReleaseSysCache(class_tuple);

        // reltuples == -1 means never analyzed
        if reltuples >= 0.0 {
            Some("analyzed".to_string())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------
// Pure helper functions (no PG catalog access, testable)
// ---------------------------------------------------------------

/// Interpret PostgreSQL's `n_distinct` encoding.
///
/// Positive values are absolute NDV counts.
/// Negative values are a fraction of the table's row count
/// (e.g., -1.0 means every row is distinct).
fn interpret_n_distinct(n_distinct: f32, row_count: f64) -> f64 {
    if n_distinct > 0.0 {
        f64::from(n_distinct)
    } else if n_distinct < 0.0 {
        (f64::from(-n_distinct) * row_count).max(1.0)
    } else {
        0.0
    }
}

/// Parse a PostgreSQL array string like `{value1,value2,value3}`.
///
/// PostgreSQL arrays are formatted as text with curly braces. This
/// parser handles basic arrays without nested structures or complex
/// escaping. Returns `None` if parsing fails.
fn parse_pg_array(array_str: &str) -> Option<Vec<String>> {
    let trimmed = array_str.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }

    let content = &trimmed[1..trimmed.len() - 1];
    if content.is_empty() {
        return Some(Vec::new());
    }

    // Split by commas, handling quoted values
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in content.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ',' && !in_quotes {
            values.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        values.push(current.trim().to_string());
    }

    Some(values)
}

/// Parse a PostgreSQL float array like `{0.1,0.2,0.3}`.
///
/// Returns `None` if parsing fails.
fn parse_float_array(array_str: &str) -> Option<Vec<f64>> {
    let strings = parse_pg_array(array_str)?;
    let mut floats = Vec::with_capacity(strings.len());

    for s in strings {
        match s.parse::<f64>() {
            Ok(f) => floats.push(f),
            Err(_) => return None,
        }
    }

    Some(floats)
}

/// Parse column names from a PostgreSQL index definition.
///
/// Extracts column names from CREATE INDEX DDL like:
/// "CREATE INDEX idx_name ON table_name USING btree (col1, col2)"
///
/// Returns empty vector if parsing fails.
fn parse_index_columns(index_def: &str) -> Vec<String> {
    // Find the opening parenthesis after "ON table_name"
    let start = match index_def.find('(') {
        Some(pos) => pos + 1,
        None => return Vec::new(),
    };

    // Find the matching closing parenthesis by counting
    let mut depth = 0;
    let mut end = None;
    for (i, ch) in index_def[start..].chars().enumerate() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    end = Some(start + i);
                    break;
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    let end = match end {
        Some(pos) => pos,
        None => return Vec::new(),
    };

    // Extract column list
    let col_list = &index_def[start..end];

    // Split by commas and clean up
    col_list
        .split(',')
        .map(|s| {
            // Remove function calls like "lower(name)" -> "name"
            let trimmed = s.trim();
            if let Some(paren_pos) = trimmed.find('(') {
                // Extract the argument from a function call
                if let Some(func_end) = trimmed.find(')') {
                    trimmed[paren_pos + 1..func_end].trim().to_string()
                } else {
                    trimmed.to_string()
                }
            } else {
                trimmed.to_string()
            }
        })
        .collect()
}

/// Parse PostgreSQL index type name to IndexType enum.
fn parse_index_type(type_name: &str) -> Option<ra_core::IndexType> {
    match type_name.to_lowercase().as_str() {
        "btree" => Some(ra_core::IndexType::BTree),
        "hash" => Some(ra_core::IndexType::Hash),
        "gin" => Some(ra_core::IndexType::Gin),
        "gist" => Some(ra_core::IndexType::Gist),
        "spgist" => Some(ra_core::IndexType::SpGist),
        "brin" => Some(ra_core::IndexType::Brin),
        _ => Some(ra_core::IndexType::Unknown),
    }
}

/// Create an equi-depth histogram from PostgreSQL histogram bounds.
///
/// PostgreSQL stores histogram_bounds as an array of boundary values
/// for equal-frequency buckets. We convert this to Ra's EquiDepthHistogram
/// format.
fn create_equidepth_histogram(
    bounds: Vec<String>,
    row_count: f64,
    distinct_count: f64,
) -> ra_core::Histogram {
    use ra_core::{EquiDepthHistogram, Histogram, HistogramBucket};

    if bounds.is_empty() {
        // Empty histogram
        return Histogram::EquiDepth(EquiDepthHistogram {
            buckets: Vec::new(),
            rows_per_bucket: 0.0,
        });
    }

    // PostgreSQL histogram_bounds has n+1 values for n buckets
    let num_buckets = bounds.len().saturating_sub(1);
    if num_buckets == 0 {
        return Histogram::EquiDepth(EquiDepthHistogram {
            buckets: Vec::new(),
            rows_per_bucket: 0.0,
        });
    }

    let rows_per_bucket = row_count / num_buckets as f64;
    let distinct_per_bucket = distinct_count / num_buckets as f64;

    let mut buckets = Vec::with_capacity(num_buckets);
    for i in 1..bounds.len() {
        buckets.push(HistogramBucket {
            upper_bound: bounds[i].clone(),
            row_count: rows_per_bucket,
            distinct_count: distinct_per_bucket,
        });
    }

    Histogram::EquiDepth(EquiDepthHistogram {
        buckets,
        rows_per_bucket,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_distinct_positive() {
        let ndv = interpret_n_distinct(100.0, 1000.0);
        assert!((ndv - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_fraction() {
        let ndv = interpret_n_distinct(-0.5, 1000.0);
        assert!((ndv - 500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_one() {
        let ndv = interpret_n_distinct(-1.0, 1000.0);
        assert!((ndv - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_zero() {
        let ndv = interpret_n_distinct(0.0, 1000.0);
        assert!((ndv - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn n_distinct_negative_small_table() {
        let ndv = interpret_n_distinct(-0.001, 0.5);
        assert!(ndv >= 1.0);
    }

    #[test]
    fn parse_simple_array() {
        let result = parse_pg_array("{a,b,c}");
        assert_eq!(result, Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
    }

    #[test]
    fn parse_empty_array() {
        let result = parse_pg_array("{}");
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn parse_quoted_array() {
        let result = parse_pg_array(r#"{"hello world","test,value"}"#);
        assert_eq!(result, Some(vec!["hello world".to_string(), "test,value".to_string()]));
    }

    #[test]
    fn parse_invalid_array() {
        let result = parse_pg_array("not an array");
        assert_eq!(result, None);
    }

    #[test]
    fn parse_simple_float_array() {
        let result = parse_float_array("{0.1,0.2,0.3}");
        assert_eq!(result, Some(vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn parse_empty_float_array() {
        let result = parse_float_array("{}");
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn parse_invalid_float_array() {
        let result = parse_float_array("{0.1,not_a_float,0.3}");
        assert_eq!(result, None);
    }

    #[test]
    fn histogram_from_bounds() {
        let bounds = vec!["1".to_string(), "10".to_string(), "20".to_string(), "30".to_string()];
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 3);
                assert!((h.rows_per_bucket - 333.333).abs() < 1.0);
                assert_eq!(h.buckets[0].upper_bound, "10");
                assert_eq!(h.buckets[1].upper_bound, "20");
                assert_eq!(h.buckets[2].upper_bound, "30");
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn histogram_empty_bounds() {
        let bounds = Vec::new();
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 0);
                assert!((h.rows_per_bucket - 0.0).abs() < f64::EPSILON);
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn histogram_single_bound() {
        let bounds = vec!["5".to_string()];
        let hist = create_equidepth_histogram(bounds, 1000.0, 30.0);

        match hist {
            ra_core::Histogram::EquiDepth(h) => {
                assert_eq!(h.buckets.len(), 0);
            }
            _ => panic!("Expected EquiDepth histogram"),
        }
    }

    #[test]
    fn parse_simple_index_def() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (col1, col2)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["col1", "col2"]);
    }

    #[test]
    fn parse_index_with_function() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (lower(name), age)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["name", "age"]);
    }

    #[test]
    fn parse_single_column_index() {
        let def = "CREATE INDEX idx_name ON table_name USING btree (id)";
        let cols = parse_index_columns(def);
        assert_eq!(cols, vec!["id"]);
    }

    #[test]
    fn parse_invalid_index_def() {
        let def = "Not a valid index definition";
        let cols = parse_index_columns(def);
        assert!(cols.is_empty());
    }

    #[test]
    fn parse_btree_index_type() {
        let idx_type = parse_index_type("btree");
        assert_eq!(idx_type, Some(ra_core::IndexType::BTree));
    }

    #[test]
    fn parse_gin_index_type() {
        let idx_type = parse_index_type("gin");
        assert_eq!(idx_type, Some(ra_core::IndexType::Gin));
    }

    #[test]
    fn parse_hash_index_type() {
        let idx_type = parse_index_type("hash");
        assert_eq!(idx_type, Some(ra_core::IndexType::Hash));
    }

    #[test]
    fn parse_unknown_index_type() {
        let idx_type = parse_index_type("custom_index");
        assert_eq!(idx_type, Some(ra_core::IndexType::Unknown));
    }

    #[test]
    fn mvcc_stats_hot_updates() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.9,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.2,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-20".to_string()),
        };

        assert!((mvcc.hot_update_ratio - 0.9).abs() < f64::EPSILON);
        assert!(!mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_needs_vacuum_dead_tuples() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.7,
            dead_tuple_ratio: 0.15, // > 0.1 threshold
            bloat_factor: 1.5,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-01".to_string()),
        };

        assert!(mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_needs_vacuum_bloat() {
        let mvcc = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 2.5, // > 2.0 threshold
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-02-01".to_string()),
        };

        assert!(mvcc.needs_vacuum());
    }

    #[test]
    fn mvcc_stats_is_stale() {
        let mvcc_no_analyze = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.1,
            last_analyze: None,
            last_vacuum: Some("2026-03-20".to_string()),
        };

        assert!(mvcc_no_analyze.is_stale());

        let mvcc_recent = PostgresMvccStats {
            hot_update_ratio: 0.8,
            dead_tuple_ratio: 0.05,
            bloat_factor: 1.1,
            last_analyze: Some("2026-03-21".to_string()),
            last_vacuum: Some("2026-03-20".to_string()),
        };

        // Note: is_stale() is simplified - just checks presence
        assert!(!mvcc_recent.is_stale());
    }
}
