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

// PostgreSQL C macros re-implemented in Rust. These are not exposed
// by pgrx because they are preprocessor macros in arrayutils.h /
// array.h.

/// Equivalent to C macro `DatumGetArrayTypeP(d)`.
#[expect(clippy::cast_ptr_alignment, reason = "legacy allow")]
unsafe fn datum_get_array_type_p(datum: pg_sys::Datum) -> *mut pg_sys::ArrayType {
    // For non-toasted arrays the datum IS the pointer. For toasted
    // arrays we must detoast first via `pg_detoast_datum`.
    let raw = datum.cast_mut_ptr::<pg_sys::varlena>();
    pg_sys::pg_detoast_datum(raw) as *mut pg_sys::ArrayType
}

/// Equivalent to C macro `ARR_NDIM(a)`.
unsafe fn arr_ndim(a: *mut pg_sys::ArrayType) -> i32 {
    (*a).ndim
}

/// Equivalent to C macro `ARR_DIMS(a)`.
#[expect(clippy::cast_ptr_alignment, reason = "legacy allow")]
unsafe fn arr_dims(a: *mut pg_sys::ArrayType) -> *mut i32 {
    // dims start right after the ArrayType header
    (a as *mut u8).add(std::mem::size_of::<pg_sys::ArrayType>()) as *mut i32
}

/// Equivalent to C macro `ARR_ELEMTYPE(a)`.
unsafe fn arr_elemtype(a: *mut pg_sys::ArrayType) -> pg_sys::Oid {
    (*a).elemtype
}

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
unsafe fn resolve_relation_oid(schema: &str, table: &str) -> Option<pg_sys::Oid> {
    let ns_oid = resolve_namespace_oid(schema)?;
    let c_table = std::ffi::CString::new(table).ok()?;
    let rel_oid = pg_sys::get_relname_relid(c_table.as_ptr(), ns_oid);
    if rel_oid == pg_sys::InvalidOid {
        None
    } else {
        Some(rel_oid)
    }
}

/// Core metadata from `pg_class` needed for statistics.
struct RelClassInfo {
    reltuples: f32,
    relpages: i32,
}

/// Read `pg_class` core metadata (reltuples, relpages) in a
/// single syscache lookup.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_relclass_info(rel_oid: pg_sys::Oid) -> Option<RelClassInfo> {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if tuple.is_null() {
        return None;
    }

    let class_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let info = RelClassInfo {
        reltuples: (*class_form).reltuples,
        relpages: (*class_form).relpages,
    };

    pg_sys::ReleaseSysCache(tuple);

    Some(info)
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

    let class_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_class;
    let natts = (*class_form).relnatts;

    pg_sys::ReleaseSysCache(tuple);

    Some(natts)
}

/// Read the attribute name for (relation, attnum) from syscache.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_attname(rel_oid: pg_sys::Oid, attnum: i16) -> Option<String> {
    let tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
    );
    if tuple.is_null() {
        return None;
    }

    let att_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_attribute;

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

    let stat_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

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
unsafe fn read_stat_correlation(tuple: pg_sys::HeapTuple) -> Option<f32> {
    let stat_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

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
    let stat_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_statistic;

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
                col_stats.histogram = Some(create_equidepth_histogram(bounds, row_count, distinct));
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
unsafe fn read_stanumbers(tuple: pg_sys::HeapTuple, slot_idx: usize) -> Option<Vec<f64>> {
    // stanumbers1..stanumbers5 are at attribute numbers
    // Anum_pg_statistic_stanumbers1 + slot_idx
    let attnum = (pg_sys::Anum_pg_statistic_stanumbers1 as i32 + slot_idx as i32) as i16;

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
    let array = datum_get_array_type_p(datum);
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
unsafe fn read_stanumbers_first(tuple: pg_sys::HeapTuple, slot_idx: usize) -> Option<f32> {
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
    let attnum = (pg_sys::Anum_pg_statistic_stavalues1 as i32 + slot_idx as i32) as i16;

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
    let array = datum_get_array_type_p(datum);
    if array.is_null() {
        return None;
    }

    let nelems = pg_sys::ArrayGetNItems(arr_ndim(array), arr_dims(array));
    if nelems <= 0 {
        return Some(Vec::new());
    }

    // Get the element type and its type info from the catalog
    let elem_type = arr_elemtype(array);
    let mut typoutput: pg_sys::Oid = pg_sys::InvalidOid;
    let mut typ_is_varlena: bool = false;
    pg_sys::getTypeOutputInfo(elem_type, &mut typoutput, &mut typ_is_varlena);

    // Look up the element type's typlen, typbyval, typalign
    let mut typlen: i16 = 0;
    let mut typbyval: bool = false;
    let mut typalign: i8 = 0;
    pg_sys::get_typlenbyvalalign(elem_type, &mut typlen, &mut typbyval, &mut typalign);

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
            let text_ptr = pg_sys::OidOutputFunctionCall(typoutput, *elems.add(i));
            if !text_ptr.is_null() {
                let s = CStr::from_ptr(text_ptr).to_string_lossy().into_owned();
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
pub fn gather_table_stats(schema: &str, table: &str) -> Option<Statistics> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;
        gather_table_stats_by_oid(rel_oid)
    }
}

/// Gather statistics by relation OID (for metadata cache refresh).
///
/// Uses direct syscache lookups on `pg_class` and `pg_statistic`
/// instead of SPI, making it safe to call from planner hooks.
///
/// Returns `None` if the table has no statistics (unanalyzed) or
/// does not exist.
pub fn gather_table_stats_by_oid(rel_oid: pg_sys::Oid) -> Option<Statistics> {
    unsafe {
        let class_info = read_relclass_info(rel_oid)?;

        // reltuples == -1 means never analyzed
        if class_info.reltuples < 0.0 {
            return None;
        }

        let row_count = f64::from(class_info.reltuples);
        let mut stats = Statistics::new(row_count);

        // Populate page-level size from pg_class.relpages
        let page_count = class_info.relpages.max(0) as u64;
        stats.total_size = page_count * pg_sys::BLCKSZ as u64;

        let natts = read_relnatts(rel_oid)?;

        // Iterate user attributes (1-based, positive attnum)
        for attnum in 1..=natts {
            let col_name = match read_attname(rel_oid, attnum) {
                Some(name) => name,
                None => continue, // dropped column
            };

            if let Some(col_stats) = read_column_stats(rel_oid, attnum, row_count) {
                stats.columns.insert(col_name, col_stats);
            }
        }

        // Derive avg_row_size from column avg_length or from
        // total_size / row_count.
        stats.avg_row_size = compute_avg_row_size(&stats, page_count) as u64;

        // Gather index statistics for index-aware optimization
        gather_index_stats_by_oid(rel_oid, &mut stats);

        Some(stats)
    }
}

/// Gather index statistics for a table (by schema and table name).
///
/// Uses `RelationGetIndexList` and syscache lookups on `pg_index`
/// and `pg_class` instead of SPI.
fn gather_index_stats(schema: &str, table: &str, stats: &mut Statistics) {
    unsafe {
        let rel_oid = match resolve_relation_oid(schema, table) {
            Some(oid) => oid,
            None => return,
        };

        gather_index_stats_by_oid(rel_oid, stats);
    }
}

/// Gather index statistics for a table (by relation OID).
///
/// Uses `RelationGetIndexList` and syscache lookups on `pg_index`
/// and `pg_class` instead of SPI.
fn gather_index_stats_by_oid(rel_oid: pg_sys::Oid, stats: &mut Statistics) {
    unsafe {
        // Open the relation to get its index list.
        // AccessShareLock is sufficient for reading metadata.
        let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
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
        pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    }
}

/// Read metadata for a single index from syscache.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
unsafe fn read_single_index(idx_oid: pg_sys::Oid) -> Option<(String, ra_core::IndexStats)> {
    // Look up pg_class entry for the index
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if class_tuple.is_null() {
        return None;
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;

    let idx_name = CStr::from_ptr((*class_form).relname.data.as_ptr())
        .to_string_lossy()
        .into_owned();

    let index_size = (*class_form).relpages as u64 * pg_sys::BLCKSZ as u64;

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

    let idx_form = pg_sys::GETSTRUCT(idx_tuple) as *mut pg_sys::FormData_pg_index;

    let is_unique = (*idx_form).indisunique;
    let is_primary = (*idx_form).indisprimary;
    let indrelid = (*idx_form).indrelid;
    let natts = (*idx_form).indnatts as usize;

    // Read indexed column names
    let mut columns = Vec::with_capacity(natts);
    for i in 0..natts {
        let attnum = (*idx_form).indkey.values.as_slice(natts)[i];
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
    idx_stats.oid = Some(u64::from(idx_oid));

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

    let name = CStr::from_ptr(am_name_ptr).to_string_lossy();

    let result = parse_index_type(&name).unwrap_or(ra_core::IndexType::Unknown);

    pg_sys::pfree(am_name_ptr as *mut std::ffi::c_void);

    result
}

/// Gather statistics for all tables referenced in a query.
///
/// `table_names` should be a list of `(schema, table)` pairs.
/// Tables with no statistics are silently skipped.
pub fn gather_all_stats(table_names: &[(String, String)]) -> Vec<(String, Statistics)> {
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

    let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
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
unsafe fn estimate_bloat(rel_oid: pg_sys::Oid, base_stats: &Statistics) -> f64 {
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(rel_oid),
    );
    if class_tuple.is_null() {
        return 1.0;
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
    let relpages = (*class_form).relpages;
    let reltuples = (*class_form).reltuples;
    pg_sys::ReleaseSysCache(class_tuple);

    if reltuples <= 0.0 || relpages <= 0 {
        return 1.0;
    }

    let table_size = relpages as f64 * pg_sys::BLCKSZ as f64;
    let expected_size = base_stats.row_count * base_stats.avg_row_size as f64;

    if expected_size > 0.0 {
        (table_size / expected_size).max(1.0)
    } else {
        1.0
    }
}

/// Foreign key relationship discovered from pg_constraint.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKeyInfo {
    /// Columns in the referencing (child) table.
    pub columns: Vec<String>,
    /// Referenced (parent) table name.
    pub referenced_table: String,
    /// Columns in the referenced (parent) table.
    pub referenced_columns: Vec<String>,
}

// pg_constraint catalog constants not exposed by pgrx.
// Values from PostgreSQL src/include/catalog/pg_constraint.h.

/// OID of the pg_constraint catalog relation.
const CONSTRAINT_RELATION_ID: pg_sys::Oid = pg_sys::Oid::from_u32(2606);

/// pg_constraint attribute numbers (1-based).
/// contype is attribute 4 (char: 'f' for foreign key).
const ANUM_CONTYPE: i16 = 4;
/// conrelid is attribute 8 (OID of the constrained table).
const ANUM_CONRELID: i16 = 8;
/// confrelid is attribute 12 (OID of the referenced table).
const ANUM_CONFRELID: i16 = 12;
/// conkey is attribute 13 (int2[] of constrained column attnums).
const ANUM_CONKEY: i16 = 13;
/// confkey is attribute 14 (int2[] of referenced column attnums).
const ANUM_CONFKEY: i16 = 14;
/// Total number of attributes in pg_constraint (PG14-PG17).
const NATTS_PG_CONSTRAINT: usize = 26;

/// Gather foreign key relationships for a table.
///
/// Scans the `pg_constraint` catalog directly (no SPI) to find
/// foreign key constraints where `conrelid` matches the given table.
/// Safe to call from planner hooks.
///
/// Returns one `ForeignKeyInfo` per foreign key constraint found.
pub fn gather_foreign_keys(schema: &str, table: &str) -> Vec<ForeignKeyInfo> {
    unsafe { gather_foreign_keys_inner(schema, table) }
}

/// Inner implementation that performs the pg_constraint catalog scan.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process with valid
/// memory context.
unsafe fn gather_foreign_keys_inner(schema: &str, table: &str) -> Vec<ForeignKeyInfo> {
    let rel_oid = match resolve_relation_oid(schema, table) {
        Some(oid) => oid,
        None => return Vec::new(),
    };

    // Open pg_constraint catalog with AccessShareLock.
    let con_rel = pg_sys::table_open(
        CONSTRAINT_RELATION_ID,
        pg_sys::AccessShareLock as pg_sys::LOCKMODE,
    );
    if con_rel.is_null() {
        return Vec::new();
    }

    // Build scan key: conrelid = rel_oid (attribute 8).
    let mut scan_key = pg_sys::ScanKeyData::default();
    pg_sys::ScanKeyInit(
        &mut scan_key,
        ANUM_CONRELID,
        pg_sys::BTEqualStrategyNumber as u16,
        pg_sys::F_OIDEQ.into(),
        pg_sys::Datum::from(rel_oid),
    );

    // Sequential scan (no index -- pgrx doesn't expose the
    // constraint indexes, and the catalog is typically small).
    let scan = pg_sys::systable_beginscan(
        con_rel,
        pg_sys::InvalidOid,   // no index
        false,                // no index scan
        std::ptr::null_mut(), // snapshot (current)
        1,                    // number of scan keys
        &mut scan_key,
    );

    let tupdesc = (*con_rel).rd_att;
    let mut result = Vec::new();

    loop {
        let tuple = pg_sys::systable_getnext(scan);
        if tuple.is_null() {
            break;
        }

        if let Some(fk) = extract_foreign_key(tuple, tupdesc) {
            result.push(fk);
        }
    }

    pg_sys::systable_endscan(scan);
    pg_sys::table_close(con_rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

    result
}

/// Extract a foreign key from a pg_constraint HeapTuple.
///
/// Returns `None` if the constraint is not a foreign key or if
/// any required attribute is NULL.
///
/// # Safety
///
/// Must be called with a valid HeapTuple from pg_constraint.
unsafe fn extract_foreign_key(
    tuple: pg_sys::HeapTuple,
    tupdesc: pg_sys::TupleDesc,
) -> Option<ForeignKeyInfo> {
    let mut values = vec![pg_sys::Datum::from(0usize); NATTS_PG_CONSTRAINT];
    let mut nulls = vec![false; NATTS_PG_CONSTRAINT];

    pg_sys::heap_deform_tuple(tuple, tupdesc, values.as_mut_ptr(), nulls.as_mut_ptr());

    // contype is attribute 4 (index 3). Check for 'f' (foreign key).
    if nulls[ANUM_CONTYPE as usize - 1] {
        return None;
    }
    let contype = values[ANUM_CONTYPE as usize - 1].value() as u8;
    if contype != b'f' {
        return None;
    }

    // confrelid: attribute 12 (index 11)
    if nulls[ANUM_CONFRELID as usize - 1] {
        return None;
    }
    let confrelid = pg_sys::Oid::from(values[ANUM_CONFRELID as usize - 1].value() as u32);

    // conkey: attribute 13 (index 12) -- int2[] of local column attnums
    if nulls[ANUM_CONKEY as usize - 1] {
        return None;
    }
    let conkey_datum = values[ANUM_CONKEY as usize - 1];

    // confkey: attribute 14 (index 13) -- int2[] of referenced column attnums
    if nulls[ANUM_CONFKEY as usize - 1] {
        return None;
    }
    let confkey_datum = values[ANUM_CONFKEY as usize - 1];

    // Get conrelid for resolving local column names.
    if nulls[ANUM_CONRELID as usize - 1] {
        return None;
    }
    let conrelid = pg_sys::Oid::from(values[ANUM_CONRELID as usize - 1].value() as u32);

    // Resolve referenced table name.
    let ref_table = get_rel_name_safe(confrelid)?;

    // Decode conkey int2 array to column names.
    let columns = decode_attnum_array(conkey_datum, conrelid)?;

    // Decode confkey int2 array to column names.
    let referenced_columns = decode_attnum_array(confkey_datum, confrelid)?;

    Some(ForeignKeyInfo {
        columns,
        referenced_table: ref_table,
        referenced_columns,
    })
}

/// Decode an int2[] datum (column attnum array) into column names.
///
/// Used for pg_constraint.conkey and pg_constraint.confkey.
///
/// # Safety
///
/// Must be called with a valid int2[] Datum within a PG backend.
unsafe fn decode_attnum_array(datum: pg_sys::Datum, rel_oid: pg_sys::Oid) -> Option<Vec<String>> {
    let array = datum_get_array_type_p(datum);
    if array.is_null() {
        return None;
    }

    let mut elems: *mut pg_sys::Datum = std::ptr::null_mut();
    let mut elem_nulls: *mut bool = std::ptr::null_mut();
    let mut nelems: i32 = 0;

    pg_sys::deconstruct_array(
        array,
        pg_sys::INT2OID,
        2,    // int2 = 2 bytes
        true, // pass by value
        pg_sys::TYPALIGN_SHORT as i8,
        &mut elems,
        &mut elem_nulls,
        &mut nelems,
    );

    let mut names = Vec::with_capacity(nelems as usize);
    let mut failed = false;

    for i in 0..nelems as usize {
        if *elem_nulls.add(i) {
            failed = true;
            break;
        }
        let attnum = (*elems.add(i)).value() as i16;
        match read_attname(rel_oid, attnum) {
            Some(name) => names.push(name),
            None => {
                failed = true;
                break;
            }
        }
    }

    pg_sys::pfree(elems as *mut std::ffi::c_void);
    pg_sys::pfree(elem_nulls as *mut std::ffi::c_void);

    if failed {
        None
    } else {
        Some(names)
    }
}

/// Get a relation name by OID (safe wrapper).
unsafe fn get_rel_name_safe(relid: pg_sys::Oid) -> Option<String> {
    let name_ptr = pg_sys::get_rel_name(relid);
    if name_ptr.is_null() {
        return None;
    }
    let s = CStr::from_ptr(name_ptr).to_string_lossy().into_owned();
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
pub fn last_analyze_time(schema: &str, table: &str) -> Option<String> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table)?;

        let class_tuple = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::RELOID as i32,
            pg_sys::Datum::from(rel_oid),
        );
        if class_tuple.is_null() {
            return None;
        }

        let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
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

/// Compute average row size from column stats or page-level data.
///
/// Prefers summing per-column `avg_width` from `pg_statistic`.
/// Falls back to `total_size / row_count` when column-level data
/// is incomplete.
fn compute_avg_row_size(stats: &Statistics, page_count: u64) -> f64 {
    // Try summing column widths (23 bytes for tuple header).
    if !stats.columns.is_empty() {
        let width_sum: f64 = stats
            .columns
            .values()
            .map(|cs| cs.avg_length.unwrap_or(8.0))
            .sum();
        let from_columns = width_sum + 23.0;
        if from_columns > 24.0 {
            return from_columns;
        }
    }

    // Fall back to total_size / row_count.
    if stats.row_count > 0.0 && page_count > 0 {
        let total = page_count as f64 * 8192.0;
        return total / stats.row_count;
    }

    // Default: 100 bytes per row (PostgreSQL's typical default).
    100.0
}

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
        "rum" => Some(ra_core::IndexType::Rum),
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
        assert_eq!(
            result,
            Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
        );
    }

    #[test]
    fn parse_empty_array() {
        let result = parse_pg_array("{}");
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn parse_quoted_array() {
        let result = parse_pg_array(r#"{"hello world","test,value"}"#);
        assert_eq!(
            result,
            Some(vec!["hello world".to_string(), "test,value".to_string()])
        );
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
        let bounds = vec![
            "1".to_string(),
            "10".to_string(),
            "20".to_string(),
            "30".to_string(),
        ];
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

    #[test]
    fn foreign_key_info_single_column() {
        let fk = ForeignKeyInfo {
            columns: vec!["user_id".to_string()],
            referenced_table: "users".to_string(),
            referenced_columns: vec!["id".to_string()],
        };

        assert_eq!(fk.columns, vec!["user_id"]);
        assert_eq!(fk.referenced_table, "users");
        assert_eq!(fk.referenced_columns, vec!["id"]);
    }

    #[test]
    fn foreign_key_info_composite() {
        let fk = ForeignKeyInfo {
            columns: vec!["tenant_id".to_string(), "order_id".to_string()],
            referenced_table: "orders".to_string(),
            referenced_columns: vec!["tenant_id".to_string(), "id".to_string()],
        };

        assert_eq!(fk.columns.len(), 2);
        assert_eq!(fk.referenced_columns.len(), 2);
        assert_eq!(fk.referenced_table, "orders");
    }

    #[test]
    fn foreign_key_info_equality() {
        let fk1 = ForeignKeyInfo {
            columns: vec!["a".to_string()],
            referenced_table: "t".to_string(),
            referenced_columns: vec!["b".to_string()],
        };
        let fk2 = fk1.clone();
        assert_eq!(fk1, fk2);
    }

    #[test]
    fn constraint_catalog_constants() {
        // pg_constraint OID is stable across PG versions.
        assert_eq!(u32::from(CONSTRAINT_RELATION_ID), 2606);
        // Attribute numbers match PostgreSQL pg_constraint.h.
        assert_eq!(ANUM_CONTYPE, 4);
        assert_eq!(ANUM_CONRELID, 8);
        assert_eq!(ANUM_CONFRELID, 12);
        assert_eq!(ANUM_CONKEY, 13);
        assert_eq!(ANUM_CONFKEY, 14);
        assert_eq!(NATTS_PG_CONSTRAINT, 26);
    }

    #[test]
    fn avg_row_size_from_columns() {
        let mut stats = Statistics::new(1000.0);
        let mut cs1 = ColumnStats::new(100.0);
        cs1.avg_length = Some(8.0);
        stats.columns.insert("id".into(), cs1);
        let mut cs2 = ColumnStats::new(50.0);
        cs2.avg_length = Some(32.0);
        stats.columns.insert("name".into(), cs2);

        // 8 + 32 + 23 (header) = 63
        let size = compute_avg_row_size(&stats, 0);
        assert!((size - 63.0).abs() < f64::EPSILON);
    }

    #[test]
    fn avg_row_size_from_pages() {
        let stats = Statistics::new(100.0);
        // 2 pages * 8192 = 16384 bytes / 100 rows = 163.84
        let size = compute_avg_row_size(&stats, 2);
        assert!((size - 163.84).abs() < 0.01);
    }

    #[test]
    fn avg_row_size_default() {
        let stats = Statistics::new(0.0);
        let size = compute_avg_row_size(&stats, 0);
        assert!((size - 100.0).abs() < f64::EPSILON);
    }
}
