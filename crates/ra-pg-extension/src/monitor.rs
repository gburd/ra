//! System fingerprint monitor: lazy refresh of system state for neural
//! optimization.
//!
//! PostgreSQL does not allow SPI calls from background threads, so we use
//! a lazy-refresh pattern: the planner hook calls [`maybe_refresh`] on each
//! invocation, which checks elapsed time and updates the [`SystemFingerprint`]
//! inline when refresh intervals expire.
//!
//! Refresh cadences:
//! - Hardware metrics (CPU, memory, I/O, buffer hit rate): every 1 second
//! - Capabilities (loaded extensions): every 30 seconds
//! - Statistics quality (staleness, coverage): every 30 seconds

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use pgrx::prelude::*;

use ra_engine::state::{capabilities, FingerprintReader, SystemFingerprint};

/// Refresh interval for hardware metrics (CPU load, memory, I/O, buffer hit).
const HARDWARE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

/// Refresh interval for capabilities and statistics quality.
const CATALOG_REFRESH_INTERVAL: Duration = Duration::from_secs(30);

/// Global fingerprint reader shared with the optimizer.
static FINGERPRINT: OnceLock<FingerprintReader> = OnceLock::new();

/// Tracks when each category was last refreshed.
struct RefreshTimestamps {
    hardware: Instant,
    catalog: Instant,
}

/// Thread-local refresh timestamps (safe because planner_hook runs in one
/// backend process).
static mut LAST_REFRESH: Option<RefreshTimestamps> = None;

/// Re-entrancy flag for [`maybe_refresh`] (single-threaded PG backend).
static mut IN_REFRESH: bool = false;

/// RAII guard clearing [`IN_REFRESH`] on every exit path of `maybe_refresh`.
struct RefreshGuard;
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        unsafe { IN_REFRESH = false };
    }
}

/// Initialize the fingerprint monitor subsystem.
///
/// Called once during `_PG_init`. Creates the shared `FingerprintReader`
/// with default values.
pub fn init() {
    FINGERPRINT
        .set(FingerprintReader::new())
        .expect("Fingerprint monitor already initialized");
}

/// Get the shared fingerprint reader for use in optimizer components.
///
/// Returns the global `FingerprintReader` that can be cloned cheaply
/// and passed into neural optimizer pipelines.
pub fn fingerprint_reader() -> &'static FingerprintReader {
    FINGERPRINT
        .get()
        .expect("Fingerprint monitor not initialized")
}

/// Check whether a refresh is due and update the fingerprint if so.
///
/// Called at the top of each planner_hook invocation. The SPI queries
/// are fast (<1ms total) so this does not measurably impact planning
/// latency.
///
/// # Safety
///
/// Must be called from within a PostgreSQL backend process with a valid
/// SPI context (inside a planner hook or transaction).
pub fn maybe_refresh() {
    // Re-entrancy guard: the SPI queries below are themselves planned through
    // this planner hook, which calls maybe_refresh again. Without this guard
    // (and because the refresh timestamp is only updated after polling) that
    // recurses into nested SPI until the backend aborts. A PG backend is
    // single-threaded, so a plain static flag is sufficient. The RAII guard
    // resets it on every return path.
    if unsafe { IN_REFRESH } {
        return;
    }
    unsafe { IN_REFRESH = true };
    let _guard = RefreshGuard;

    let now = Instant::now();

    // Initialize timestamps on first call.
    let timestamps = unsafe {
        LAST_REFRESH.get_or_insert_with(|| RefreshTimestamps {
            hardware: Instant::now() - HARDWARE_REFRESH_INTERVAL,
            catalog: Instant::now() - CATALOG_REFRESH_INTERVAL,
        })
    };

    let reader = match FINGERPRINT.get() {
        Some(r) => r,
        None => return,
    };

    let mut fp = reader.read();
    let mut updated = false;

    // Hardware metrics: refresh every 1s.
    if now.duration_since(timestamps.hardware) >= HARDWARE_REFRESH_INTERVAL {
        let (cpu, mem, io, hit_rate) = poll_hardware_metrics();
        fp.cpu_load_fraction = cpu;
        fp.memory_pressure = mem;
        fp.io_saturation = io;
        fp.shared_buffers_hit_rate = hit_rate;
        timestamps.hardware = now;
        updated = true;
    }

    // Catalog-derived data: refresh every 30s.
    if now.duration_since(timestamps.catalog) >= CATALOG_REFRESH_INTERVAL {
        fp.capabilities = poll_capabilities();

        let (avg_stale, worst_stale, coverage) = poll_statistics_quality();
        fp.avg_staleness = avg_stale;
        fp.worst_staleness = worst_stale;
        fp.stats_coverage = coverage;

        timestamps.catalog = now;
        updated = true;
    }

    if updated {
        reader.update(fp);
    }
}

/// Query PostgreSQL for buffer cache hit rate as a proxy for I/O pressure.
///
/// Returns `(cpu_load, memory_pressure, io_saturation, buffer_hit_rate)`.
/// All values normalized to [0.0, 1.0].
fn poll_hardware_metrics() -> (f32, f32, f32, f32) {
    // Buffer hit rate from pg_stat_database for the current database.
    let buffer_hit_rate = Spi::connect(|client| {
        let result = client.select(
            "SELECT blks_hit::float8 / GREATEST(blks_hit + blks_read, 1) \
             FROM pg_stat_database WHERE datname = current_database()",
            None,
            &[],
        );
        match result {
            Ok(table) => table.first().get::<f64>(1).ok().flatten().unwrap_or(0.99),
            Err(_) => 0.99,
        }
    });

    // Estimate I/O saturation from checkpoint and backend write activity.
    // A high ratio of buffers_backend to buffers_clean indicates I/O pressure.
    let io_saturation = Spi::connect(|client| {
        let result = client.select(
            "SELECT CASE \
                WHEN (buffers_backend + buffers_clean + buffers_checkpoint) = 0 THEN 0.0 \
                ELSE buffers_backend::float8 / \
                     (buffers_backend + buffers_clean + buffers_checkpoint)::float8 \
             END \
             FROM pg_stat_bgwriter",
            None,
            &[],
        );
        match result {
            Ok(table) => table.first().get::<f64>(1).ok().flatten().unwrap_or(0.0) as f32,
            Err(_) => 0.0,
        }
    });

    // CPU and memory pressure are not directly available from PostgreSQL
    // catalogs. We approximate memory pressure from cache hit rate: low hit
    // rate implies shared_buffers is under pressure.
    let memory_pressure = (1.0 - buffer_hit_rate as f32).clamp(0.0, 1.0);

    // CPU load approximated from active backends vs max_connections.
    let cpu_load = Spi::connect(|client| {
        let result = client.select(
            "SELECT count(*)::float8 / \
                    GREATEST(current_setting('max_connections')::float8, 1) \
             FROM pg_stat_activity WHERE state = 'active'",
            None,
            &[],
        );
        match result {
            Ok(table) => table.first().get::<f64>(1).ok().flatten().unwrap_or(0.0) as f32,
            Err(_) => 0.0,
        }
    });

    (
        cpu_load.clamp(0.0, 1.0),
        memory_pressure,
        io_saturation.clamp(0.0, 1.0),
        (buffer_hit_rate as f32).clamp(0.0, 1.0),
    )
}

/// Query pg_extension for loaded extensions and map to capability bits.
///
/// Also checks runtime GUCs that indicate feature availability
/// (e.g., `max_parallel_workers_per_gather > 0` for parallel query).
fn poll_capabilities() -> u64 {
    let mut caps: u64 = 0;

    // Query loaded extensions.
    Spi::connect(|client| {
        let result = client.select("SELECT extname FROM pg_extension", None, &[]);
        if let Ok(table) = result {
            for row in table {
                if let Ok(Some(name)) = row.get::<&str>(1) {
                    match name {
                        "citus" => caps |= capabilities::CITUS,
                        "postgis" | "postgis_topology" => {
                            caps |= capabilities::POSTGIS;
                        }
                        "pg_trgm" => caps |= capabilities::PG_TRGM,
                        "documentdb" => caps |= capabilities::DOCUMENTDB,
                        "vector" => caps |= capabilities::PGVECTOR,
                        "timescaledb" => caps |= capabilities::TIMESCALEDB,
                        "pg_partman" => caps |= capabilities::PG_PARTMAN,
                        "pg_strom" => caps |= capabilities::GPU_ACCEL,
                        "rum" => caps |= capabilities::RUM_INDEX,
                        "pg_stat_statements" => {
                            caps |= capabilities::PG_STAT_STATEMENTS;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    // Check parallel query support via GUC.
    Spi::connect(|client| {
        let result = client.select(
            "SELECT current_setting('max_parallel_workers_per_gather')::int",
            None,
            &[],
        );
        if let Ok(table) = result {
            if let Ok(Some(val)) = table.first().get::<i32>(1) {
                if val > 0 {
                    caps |= capabilities::PARALLEL_QUERY;
                }
            }
        }
    });

    // Check PG version features.
    Spi::connect(|client| {
        let result = client.select(
            "SELECT current_setting('server_version_num')::int",
            None,
            &[],
        );
        if let Ok(table) = result {
            if let Ok(Some(version)) = table.first().get::<i32>(1) {
                if version >= 130000 {
                    caps |= capabilities::INCREMENTAL_SORT;
                }
                if version >= 140000 {
                    caps |= capabilities::MEMOIZE;
                }
            }
        }
    });

    // Check for active FDW usage.
    Spi::connect(|client| {
        let result = client.select(
            "SELECT EXISTS(SELECT 1 FROM pg_foreign_table LIMIT 1)",
            None,
            &[],
        );
        if let Ok(table) = result {
            if let Ok(Some(true)) = table.first().get::<bool>(1) {
                caps |= capabilities::FDW_ACTIVE;
            }
        }
    });

    caps
}

/// Query catalog for statistics freshness and coverage.
///
/// Returns `(avg_staleness, worst_staleness, stats_coverage)`, all in [0.0, 1.0].
///
/// Staleness is computed as the fraction of time since last analyze relative
/// to a 24-hour window (capped at 1.0). Coverage is the fraction of user
/// table columns that have entries in `pg_statistic`.
fn poll_statistics_quality() -> (f32, f32, f32) {
    // Staleness: time since last analyze for user tables.
    let (avg_staleness, worst_staleness) = Spi::connect(|client| {
        let result = client.select(
            "SELECT \
                COALESCE(AVG(EXTRACT(EPOCH FROM (now() - last_analyze)) / 86400.0), 1.0), \
                COALESCE(MAX(EXTRACT(EPOCH FROM (now() - last_analyze)) / 86400.0), 1.0) \
             FROM pg_stat_user_tables \
             WHERE last_analyze IS NOT NULL",
            None,
            &[],
        );
        match result {
            Ok(table) => {
                let row = table.first();
                let avg = row
                    .get::<f64>(1)
                    .ok()
                    .flatten()
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0) as f32;
                let worst = row
                    .get::<f64>(2)
                    .ok()
                    .flatten()
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0) as f32;
                (avg, worst)
            }
            Err(_) => (1.0, 1.0),
        }
    });

    // Coverage: fraction of columns with pg_statistic entries.
    let stats_coverage = Spi::connect(|client| {
        let result = client.select(
            "SELECT CASE WHEN total_cols = 0 THEN 1.0 \
                    ELSE stats_cols::float8 / total_cols::float8 END \
             FROM ( \
                SELECT \
                    (SELECT count(*) FROM pg_attribute a \
                     JOIN pg_class c ON a.attrelid = c.oid \
                     JOIN pg_namespace n ON c.relnamespace = n.oid \
                     WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') \
                       AND c.relkind = 'r' \
                       AND a.attnum > 0 \
                       AND NOT a.attisdropped) AS total_cols, \
                    (SELECT count(*) FROM pg_statistic s \
                     JOIN pg_class c ON s.starelid = c.oid \
                     JOIN pg_namespace n ON c.relnamespace = n.oid \
                     WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') \
                       AND c.relkind = 'r') AS stats_cols \
             ) sub",
            None,
            &[],
        );
        match result {
            Ok(table) => table
                .first()
                .get::<f64>(1)
                .ok()
                .flatten()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0) as f32,
            Err(_) => 0.0,
        }
    });

    (avg_staleness, worst_staleness, stats_coverage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_intervals_are_positive() {
        assert!(HARDWARE_REFRESH_INTERVAL.as_secs() > 0);
        assert!(CATALOG_REFRESH_INTERVAL.as_secs() > 0);
        assert!(CATALOG_REFRESH_INTERVAL > HARDWARE_REFRESH_INTERVAL);
    }
}
