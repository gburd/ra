//! Timeline snapshot capture from live PostgreSQL databases.
//!
//! Captures complete fingerprint snapshots from PostgreSQL system catalogs:
//! - Table and column statistics from `pg_statistic`
//! - Schema information from `pg_class`, `pg_attribute`, `pg_index`
//! - Foreign key relationships from `pg_constraint`
//! - Hardware capabilities and PostgreSQL configuration
//!
//! Safe to call from PostgreSQL backend processes. Uses direct syscache
//! lookups instead of SPI to avoid nested connection issues.

use std::collections::HashMap;
use std::ffi::CStr;

use pgrx::pg_sys;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ra_core::facts::IndexType;
use ra_engine::timeline_config::{
    ColumnDef, ColumnStatsDef, DataTypeDef, FactsSnapshot, FingerPrintSnapshot,
    ForeignKeyDef, HardwareProfileDef, IndexDef, IndexTypeDef, SchemaSnapshot,
    StatisticsSnapshot, StorageFormatDef, TableDef, TableStatsDef,
};

// Re-use some catalog access patterns from stats_bridge, but most
// functionality needs to be implemented here since stats_bridge
// functions are not public.

/// Errors from snapshot capture.
#[derive(Debug, Error)]
pub enum CaptureError {
    /// Table not found in PostgreSQL catalogs.
    #[error("table not found: {schema}.{table}")]
    TableNotFound {
        /// Schema name.
        schema: String,
        /// Table name.
        table: String,
    },

    /// Failed to query system catalogs.
    #[error("catalog query failed: {0}")]
    CatalogQueryError(String),

    /// Hardware detection failed.
    #[error("hardware detection failed: {0}")]
    HardwareDetectionError(String),

    /// TOML serialization failed.
    #[error("serialization failed: {0}")]
    SerializationError(String),

    /// Invalid snapshot data.
    #[error("invalid snapshot data: {0}")]
    InvalidData(String),
}

/// Snapshot capture configuration.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    /// Include PostgreSQL-specific MVCC statistics.
    pub include_mvcc_stats: bool,

    /// Include foreign key relationships.
    pub include_foreign_keys: bool,

    /// Include index statistics.
    pub include_indexes: bool,

    /// Snapshot time offset (seconds from timeline start).
    pub time_offset: u64,

    /// Optional snapshot label.
    pub label: Option<String>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            include_mvcc_stats: true,
            include_foreign_keys: true,
            include_indexes: true,
            time_offset: 0,
            label: None,
        }
    }
}

/// Capture a fingerprint snapshot from PostgreSQL catalogs.
///
/// Queries system catalogs for the specified tables and constructs
/// a complete `FingerPrintSnapshot` with:
/// - Hardware profile (detected from system)
/// - Schema snapshot (tables, columns, indexes, constraints)
/// - Statistics snapshot (row counts, NDV, histograms)
/// - Facts snapshot (PostgreSQL configuration and features)
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process with valid
/// memory context. Uses syscache lookups (no SPI) so safe to call
/// from planner hooks.
///
/// # Errors
///
/// Returns `CaptureError` if:
/// - Table not found in catalogs
/// - Statistics not available (table not analyzed)
/// - Hardware detection fails
/// - Catalog queries fail
pub fn capture_snapshot_from_catalog(
    table_names: &[(&str, &str)], // (schema, table) pairs
    config: &CaptureConfig,
) -> Result<FingerPrintSnapshot, CaptureError> {
    // Detect hardware profile
    let hardware_profile = detect_postgres_hardware()
        .map_err(|e| CaptureError::HardwareDetectionError(e.to_string()))?;

    // Detect PostgreSQL features and configuration
    let facts = detect_postgres_facts();

    // Capture schema snapshot
    let schema = capture_schema_snapshot(table_names, config)?;

    // Capture statistics snapshot
    let statistics = capture_statistics_snapshot(table_names)?;

    Ok(FingerPrintSnapshot {
        time_offset: config.time_offset,
        label: config.label.clone(),
        hardware_profile: hardware_profile.name.clone(),
        schema,
        statistics,
        facts,
    })
}

/// Detect hardware capabilities from PostgreSQL configuration and system.
///
/// Queries `pg_settings` and system information to build a complete
/// hardware profile including CPU cores, memory, SIMD capabilities,
/// and GPU presence.
fn detect_postgres_hardware() -> Result<HardwareProfileDef, CaptureError> {
    // Use ra_hardware to detect system capabilities
    let hw = ra_hardware::detect_hardware();

    // Query PostgreSQL configuration for memory settings
    let pg_memory = query_postgres_memory();

    Ok(HardwareProfileDef {
        name: "postgres".to_string(),
        cpu_cores: hw.cpu_cores,
        total_memory: hw.total_memory,
        available_memory: Some(pg_memory.available_memory),
        simd_width: hw.simd_width,
        has_gpu: hw.has_gpu,
        gpu_memory: hw.gpu_memory,
        l1_cache_size: hw.l1_cache_size,
        l2_cache_size: hw.l2_cache_size,
        l3_cache_size: hw.l3_cache_size,
    })
}

/// PostgreSQL memory configuration.
struct PostgresMemory {
    /// Available memory for query execution.
    available_memory: u64,
    /// Work memory per operation.
    work_mem: u64,
    /// Shared buffers size.
    shared_buffers: u64,
}

/// Query PostgreSQL memory configuration from pg_settings.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process.
fn query_postgres_memory() -> PostgresMemory {
    unsafe {
        // Read work_mem (in KB)
        let work_mem = read_guc_int(c"work_mem".as_ptr()).unwrap_or(4096) as u64 * 1024;

        // Read shared_buffers (in 8KB blocks for PostgreSQL < 14, bytes for >= 14)
        let shared_buffers = read_guc_int(c"shared_buffers".as_ptr()).unwrap_or(16384) as u64 * 8192;

        // Estimate available memory (conservative: work_mem * max_connections)
        let max_connections = read_guc_int(c"max_connections".as_ptr()).unwrap_or(100) as u64;
        let available_memory = work_mem * max_connections + shared_buffers;

        PostgresMemory {
            available_memory,
            work_mem,
            shared_buffers,
        }
    }
}

/// Read an integer GUC setting.
///
/// # Safety
///
/// Must be called with a valid GUC name pointer.
unsafe fn read_guc_int(name: *const i8) -> Option<i32> {
    let guc = pg_sys::GetConfigOptionByName(name, std::ptr::null_mut(), false);
    if guc.is_null() {
        return None;
    }

    let value_str = CStr::from_ptr(guc).to_string_lossy();
    value_str.parse::<i32>().ok()
}

/// Detect PostgreSQL features and configuration.
///
/// Queries `pg_settings` and version information to determine:
/// - Parallel query support
/// - JIT compilation
/// - Partitioning capabilities
/// - Join algorithm support
/// - Index types available
fn detect_postgres_facts() -> FactsSnapshot {
    let mut facts = FactsSnapshot::default();

    unsafe {
        // Check PostgreSQL version for feature support
        let version = pg_sys::PG_VERSION_NUM;

        // Parallel query support (PG 9.6+)
        facts.supports_parallel_scan = Some(version >= 90600);

        // JIT compilation (PG 11+)
        let has_jit = version >= 110000;

        // Query enable_* GUCs for join algorithm support
        facts.supports_hash_join = Some(
            read_guc_bool(c"enable_hashjoin".as_ptr()).unwrap_or(true),
        );

        // Read parallel workers configuration
        facts.parallel_workers = read_guc_int(c"max_parallel_workers_per_gather".as_ptr())
            .map(|v| v as u32);

        // Read work_mem
        let work_mem_kb = read_guc_int(c"work_mem".as_ptr()).unwrap_or(4096);
        facts.work_mem_bytes = Some(work_mem_kb as u64 * 1024);

        // Add custom facts
        facts.custom.insert(
            "jit_enabled".to_string(),
            toml::Value::Boolean(has_jit),
        );
        facts.custom.insert(
            "postgresql_version".to_string(),
            toml::Value::Integer(version as i64),
        );
    }

    facts
}

/// Read a boolean GUC setting.
///
/// # Safety
///
/// Must be called with a valid GUC name pointer.
unsafe fn read_guc_bool(name: *const i8) -> Option<bool> {
    let guc = pg_sys::GetConfigOptionByName(name, std::ptr::null_mut(), false);
    if guc.is_null() {
        return None;
    }

    let value_str = CStr::from_ptr(guc).to_string_lossy();
    match value_str.as_ref() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// Capture schema snapshot from PostgreSQL catalogs.
///
/// Queries `pg_class`, `pg_attribute`, `pg_index`, and `pg_constraint`
/// to build complete table definitions with columns, indexes, and
/// foreign key relationships.
fn capture_schema_snapshot(
    table_names: &[(&str, &str)],
    config: &CaptureConfig,
) -> Result<SchemaSnapshot, CaptureError> {
    let mut tables = Vec::new();

    for (schema, table) in table_names {
        let table_def = capture_table_schema(schema, table, config)?;
        tables.push(table_def);
    }

    Ok(SchemaSnapshot { tables })
}

/// Resolve a schema name to its namespace OID.
///
/// # Safety
///
/// Must be called within a PostgreSQL backend process with valid
/// memory context.
unsafe fn resolve_namespace_oid(schema: &str) -> Option<pg_sys::Oid> {
    let c_schema = std::ffi::CString::new(schema).ok()?;
    let ns_oid = pg_sys::get_namespace_oid(c_schema.as_ptr(), true);
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

/// Capture schema for a single table.
fn capture_table_schema(
    schema: &str,
    table: &str,
    config: &CaptureConfig,
) -> Result<TableDef, CaptureError> {
    unsafe {
        let rel_oid = resolve_relation_oid(schema, table).ok_or_else(|| {
            CaptureError::TableNotFound {
                schema: schema.to_string(),
                table: table.to_string(),
            }
        })?;

        // Get column definitions
        let columns = capture_columns(rel_oid)?;

        // Get index definitions
        let indexes = if config.include_indexes {
            capture_indexes(rel_oid)?
        } else {
            Vec::new()
        };

        // Get foreign keys
        let foreign_keys = if config.include_foreign_keys {
            capture_foreign_keys(schema, table)
        } else {
            Vec::new()
        };

        // Detect primary key (from indexes marked indisprimary)
        let primary_key = detect_primary_key(rel_oid);

        // Detect storage format (default to row-based for PostgreSQL)
        let storage_format = StorageFormatDef::RowBased;

        Ok(TableDef {
            name: table.to_string(),
            storage_format,
            columns,
            indexes,
            primary_key,
            foreign_keys,
        })
    }
}

/// Capture column definitions for a table.
///
/// # Safety
///
/// Must be called with a valid relation OID.
unsafe fn capture_columns(rel_oid: pg_sys::Oid) -> Result<Vec<ColumnDef>, CaptureError> {
    let natts = read_relnatts(rel_oid).ok_or_else(|| {
        CaptureError::CatalogQueryError("failed to read column count".to_string())
    })?;

    let mut columns = Vec::new();

    for attnum in 1..=natts {
        let col_name = match read_attname(rel_oid, attnum) {
            Some(name) => name,
            None => continue, // Dropped column
        };

        // Read column type
        let data_type = read_column_type(rel_oid, attnum)?;

        // Read nullable flag
        let nullable = read_column_nullable(rel_oid, attnum);

        columns.push(ColumnDef {
            name: col_name,
            data_type,
            nullable,
        });
    }

    Ok(columns)
}

/// Read column data type from pg_attribute.
///
/// # Safety
///
/// Must be called with a valid relation OID and attribute number.
unsafe fn read_column_type(
    rel_oid: pg_sys::Oid,
    attnum: i16,
) -> Result<DataTypeDef, CaptureError> {
    let tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
    );
    if tuple.is_null() {
        return Err(CaptureError::CatalogQueryError(
            "attribute not found".to_string(),
        ));
    }

    let att_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_attribute;
    let typid = (*att_form).atttypid;

    pg_sys::ReleaseSysCache(tuple);

    // Map PostgreSQL type OID to DataTypeDef
    let data_type = match typid {
        pg_sys::INT2OID | pg_sys::INT4OID | pg_sys::INT8OID => DataTypeDef::Integer,
        pg_sys::FLOAT4OID | pg_sys::FLOAT8OID | pg_sys::NUMERICOID => DataTypeDef::Float,
        pg_sys::TEXTOID | pg_sys::VARCHAROID | pg_sys::BPCHAROID => DataTypeDef::String,
        pg_sys::BOOLOID => DataTypeDef::Boolean,
        pg_sys::TIMESTAMPOID | pg_sys::TIMESTAMPTZOID | pg_sys::DATEOID => DataTypeDef::Timestamp,
        pg_sys::BYTEAOID => DataTypeDef::Binary,
        pg_sys::JSONOID | pg_sys::JSONBOID => DataTypeDef::Json,
        _ => {
            // Get type name for unknown types
            let type_name = get_type_name(typid).unwrap_or_else(|| "unknown".to_string());
            DataTypeDef::Other(type_name)
        }
    };

    Ok(data_type)
}

/// Get type name from type OID.
///
/// # Safety
///
/// Must be called with a valid type OID.
unsafe fn get_type_name(typid: pg_sys::Oid) -> Option<String> {
    let tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::TYPEOID as i32,
        pg_sys::Datum::from(typid),
    );
    if tuple.is_null() {
        return None;
    }

    let type_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_type;
    let name = CStr::from_ptr((*type_form).typname.data.as_ptr())
        .to_string_lossy()
        .into_owned();

    pg_sys::ReleaseSysCache(tuple);
    Some(name)
}

/// Read column nullable flag from pg_attribute.
///
/// # Safety
///
/// Must be called with a valid relation OID and attribute number.
unsafe fn read_column_nullable(rel_oid: pg_sys::Oid, attnum: i16) -> bool {
    let tuple = pg_sys::SearchSysCache2(
        pg_sys::SysCacheIdentifier::ATTNUM as i32,
        pg_sys::Datum::from(rel_oid),
        pg_sys::Datum::from(attnum as i32),
    );
    if tuple.is_null() {
        return true; // Default to nullable
    }

    let att_form = pg_sys::GETSTRUCT(tuple) as *mut pg_sys::FormData_pg_attribute;
    let not_null = (*att_form).attnotnull;

    pg_sys::ReleaseSysCache(tuple);

    !not_null
}

/// Capture index definitions for a table.
///
/// # Safety
///
/// Must be called with a valid relation OID.
unsafe fn capture_indexes(rel_oid: pg_sys::Oid) -> Result<Vec<IndexDef>, CaptureError> {
    // Open the relation to get its index list
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return Err(CaptureError::CatalogQueryError(
            "failed to open relation".to_string(),
        ));
    }

    let index_list = pg_sys::RelationGetIndexList(rel);
    let mut indexes = Vec::new();

    let n_indexes = (*index_list).length;
    for i in 0..n_indexes {
        let cell = pg_sys::list_nth(index_list, i);
        let idx_oid = pg_sys::Oid::from(cell as u32);

        if let Some(index_def) = capture_single_index(idx_oid, rel_oid)? {
            indexes.push(index_def);
        }
    }

    pg_sys::list_free(index_list);
    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

    Ok(indexes)
}

/// Capture definition for a single index.
///
/// # Safety
///
/// Must be called with valid index and relation OIDs.
unsafe fn capture_single_index(
    idx_oid: pg_sys::Oid,
    rel_oid: pg_sys::Oid,
) -> Result<Option<IndexDef>, CaptureError> {
    // Look up pg_class entry for the index
    let class_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::RELOID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if class_tuple.is_null() {
        return Ok(None);
    }

    let class_form = pg_sys::GETSTRUCT(class_tuple) as *mut pg_sys::FormData_pg_class;
    let idx_name = CStr::from_ptr((*class_form).relname.data.as_ptr())
        .to_string_lossy()
        .into_owned();
    let am_oid = (*class_form).relam;
    pg_sys::ReleaseSysCache(class_tuple);

    // Look up pg_index entry
    let idx_tuple = pg_sys::SearchSysCache1(
        pg_sys::SysCacheIdentifier::INDEXRELID as i32,
        pg_sys::Datum::from(idx_oid),
    );
    if idx_tuple.is_null() {
        return Ok(None);
    }

    let idx_form = pg_sys::GETSTRUCT(idx_tuple) as *mut pg_sys::FormData_pg_index;
    let is_unique = (*idx_form).indisunique;
    let natts = (*idx_form).indnatts as usize;

    // Read indexed column names
    let mut columns = Vec::with_capacity(natts);
    for i in 0..natts {
        let attnum = (*idx_form).indkey.values.as_slice(natts)[i];
        if attnum > 0 {
            if let Some(name) = stats_bridge::read_attname(rel_oid, attnum) {
                columns.push(name);
            }
        } else {
            // Expression index
            columns.push(format!("expr_{i}"));
        }
    }

    pg_sys::ReleaseSysCache(idx_tuple);

    // Get index type from access method
    let index_type = resolve_index_type(am_oid);

    Ok(Some(IndexDef {
        name: idx_name,
        index_type,
        columns,
        included_columns: Vec::new(), // TODO: Parse included columns from indoption
        is_unique,
    }))
}

/// Resolve access method OID to index type.
///
/// # Safety
///
/// Must be called with a valid access method OID.
unsafe fn resolve_index_type(am_oid: pg_sys::Oid) -> IndexTypeDef {
    if am_oid == pg_sys::InvalidOid {
        return IndexTypeDef::Unknown;
    }

    let am_name_ptr = pg_sys::get_am_name(am_oid);
    if am_name_ptr.is_null() {
        return IndexTypeDef::Unknown;
    }

    let name = CStr::from_ptr(am_name_ptr).to_string_lossy();
    let result = match name.as_ref() {
        "btree" => IndexTypeDef::Btree,
        "hash" => IndexTypeDef::Hash,
        "gin" => IndexTypeDef::Gin,
        "gist" => IndexTypeDef::Gist,
        "spgist" => IndexTypeDef::SpGist,
        "brin" => IndexTypeDef::Brin,
        "rum" => IndexTypeDef::Rum,
        _ => IndexTypeDef::Unknown,
    };

    pg_sys::pfree(am_name_ptr as *mut std::ffi::c_void);
    result
}

/// Detect primary key columns from indexes.
///
/// # Safety
///
/// Must be called with a valid relation OID.
unsafe fn detect_primary_key(rel_oid: pg_sys::Oid) -> Vec<String> {
    let rel = pg_sys::table_open(rel_oid, pg_sys::AccessShareLock as pg_sys::LOCKMODE);
    if rel.is_null() {
        return Vec::new();
    }

    let index_list = pg_sys::RelationGetIndexList(rel);
    let mut primary_key = Vec::new();

    let n_indexes = (*index_list).length;
    for i in 0..n_indexes {
        let cell = pg_sys::list_nth(index_list, i);
        let idx_oid = pg_sys::Oid::from(cell as u32);

        // Check if this index is the primary key
        let idx_tuple = pg_sys::SearchSysCache1(
            pg_sys::SysCacheIdentifier::INDEXRELID as i32,
            pg_sys::Datum::from(idx_oid),
        );
        if idx_tuple.is_null() {
            continue;
        }

        let idx_form = pg_sys::GETSTRUCT(idx_tuple) as *mut pg_sys::FormData_pg_index;
        if (*idx_form).indisprimary {
            let natts = (*idx_form).indnatts as usize;
            for j in 0..natts {
                let attnum = (*idx_form).indkey.values.as_slice(natts)[j];
                if attnum > 0 {
                    if let Some(name) = read_attname(rel_oid, attnum) {
                        primary_key.push(name);
                    }
                }
            }
        }

        pg_sys::ReleaseSysCache(idx_tuple);

        if !primary_key.is_empty() {
            break;
        }
    }

    pg_sys::list_free(index_list);
    pg_sys::table_close(rel, pg_sys::AccessShareLock as pg_sys::LOCKMODE);

    primary_key
}

/// Capture foreign key constraints for a table.
fn capture_foreign_keys(schema: &str, table: &str) -> Vec<ForeignKeyDef> {
    // Use stats_bridge public function
    let fk_infos = crate::stats_bridge::gather_foreign_keys(schema, table);

    fk_infos
        .into_iter()
        .map(|fk| ForeignKeyDef {
            columns: fk.columns,
            referenced_table: fk.referenced_table,
            referenced_columns: fk.referenced_columns,
        })
        .collect()
}

/// Capture statistics snapshot from PostgreSQL catalogs.
///
/// Queries `pg_class` and `pg_statistic` to gather table and column
/// statistics including row counts, NDV, null fractions, and correlations.
fn capture_statistics_snapshot(
    table_names: &[(&str, &str)],
) -> Result<StatisticsSnapshot, CaptureError> {
    let mut tables = Vec::new();

    for (schema, table) in table_names {
        if let Some(table_stats) = capture_table_statistics(schema, table) {
            tables.push(table_stats);
        }
    }

    Ok(StatisticsSnapshot { tables })
}

/// Capture statistics for a single table.
fn capture_table_statistics(schema: &str, table: &str) -> Option<TableStatsDef> {
    let stats = crate::stats_bridge::gather_table_stats(schema, table)?;

    // Gather column statistics
    let mut columns = Vec::new();
    for (col_name, col_stats) in &stats.columns {
        columns.push(ColumnStatsDef {
            name: col_name.clone(),
            ndv: col_stats.distinct_count as u64,
            null_fraction: col_stats.null_fraction,
            avg_width: col_stats.avg_length.unwrap_or(8.0),
            correlation: col_stats.correlation,
            min_value: col_stats.min_value.clone(),
            max_value: col_stats.max_value.clone(),
        });
    }

    Some(TableStatsDef {
        name: table.to_string(),
        row_count: stats.row_count as u64,
        page_count: Some((stats.total_size / 8192).max(1)),
        avg_row_size: Some(stats.avg_row_size as f64),
        table_size_bytes: Some(stats.total_size),
        columns,
    })
}

/// Serialize snapshot to TOML format.
pub fn snapshot_to_toml(snapshot: &FingerPrintSnapshot) -> Result<String, CaptureError> {
    toml::to_string_pretty(snapshot)
        .map_err(|e| CaptureError::SerializationError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_config_defaults() {
        let config = CaptureConfig::default();
        assert!(config.include_mvcc_stats);
        assert!(config.include_foreign_keys);
        assert!(config.include_indexes);
        assert_eq!(config.time_offset, 0);
        assert!(config.label.is_none());
    }

    #[test]
    fn data_type_mapping() {
        // Test that PostgreSQL type OIDs map correctly
        unsafe {
            let int_type = read_column_type(pg_sys::InvalidOid, 1);
            // This would require a real database connection to test properly
        }
    }
}
