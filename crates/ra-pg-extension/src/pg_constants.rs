//! PostgreSQL system constants.
//!
//! Defines named constants for PostgreSQL configuration parameters,
//! cost model defaults, and system values. Using named constants instead
//! of magic numbers improves maintainability and prevents errors.
//!
//! Hardware-aware functions use the detected hardware profile to provide
//! optimal cost parameters for the actual system configuration.

/// PostgreSQL default cost parameters.
///
/// These match PostgreSQL's built-in defaults and are used for
/// cost calibration and GUC manipulation.
pub mod cost_defaults {
    /// Sequential page fetch cost (baseline unit).
    pub const SEQ_PAGE_COST: f64 = 1.0;

    /// Random page fetch cost (HDD default).
    ///
    /// PostgreSQL default assumes spinning disks. Modern SSDs
    /// typically use 1.0-1.5.
    pub const RANDOM_PAGE_COST: f64 = 4.0;

    /// Cost to process one tuple (row).
    pub const CPU_TUPLE_COST: f64 = 0.01;

    /// Cost to process one index tuple.
    pub const CPU_INDEX_TUPLE_COST: f64 = 0.005;

    /// Cost of a comparison operator.
    pub const CPU_OPERATOR_COST: f64 = 0.0025;
}

/// PostgreSQL GUC parameter tuning values.
///
/// These are strategic values used to manipulate the planner's
/// behavior via cost-based guidance.
pub mod guc_tuning {
    /// Low random_page_cost for SSDs - favors index scans.
    ///
    /// Modern SSDs have minimal seek penalty, so random access
    /// is nearly as cheap as sequential.
    pub const RANDOM_PAGE_COST_SSD: f64 = 1.0;

    /// High random_page_cost - strongly favors sequential scans.
    ///
    /// Used to discourage index scans when we know seq scan is better.
    pub const RANDOM_PAGE_COST_FORCE_SEQSCAN: f64 = 10.0;
}

/// Rough cardinality estimation constants.
///
/// Used for initial cost estimates when detailed statistics aren't available.
pub mod estimation {
    /// Typical rows per page (8KB page, ~80 byte rows).
    pub const ROWS_PER_PAGE: f64 = 100.0;

    /// Default row count when no statistics available.
    pub const DEFAULT_ROW_COUNT: f64 = 1000.0;

    /// Average bytes per row (rough estimate for memory usage).
    pub const BYTES_PER_ROW: f64 = 100.0;
}

/// GUC parameter names.
///
/// Centralizes string literals for GUC names to avoid typos.
pub mod guc_names {
    pub const ENABLE_HASHJOIN: &str = "enable_hashjoin";
    pub const ENABLE_MERGEJOIN: &str = "enable_mergejoin";
    pub const ENABLE_NESTLOOP: &str = "enable_nestloop";
    pub const ENABLE_SEQSCAN: &str = "enable_seqscan";
    pub const ENABLE_INDEXSCAN: &str = "enable_indexscan";
    pub const ENABLE_BITMAPSCAN: &str = "enable_bitmapscan";
    pub const RANDOM_PAGE_COST: &str = "random_page_cost";
}

/// PostgreSQL built-in type OIDs.
///
/// Well-known type identifiers from PostgreSQL's pg_type catalog.
pub mod type_oids {
    pub const BOOLOID: u32 = 16;
    pub const INT2OID: u32 = 21;
    pub const INT4OID: u32 = 23;
    pub const INT8OID: u32 = 20;
    pub const FLOAT4OID: u32 = 700;
    pub const FLOAT8OID: u32 = 701;
    pub const TEXTOID: u32 = 25;
    pub const VARCHAROID: u32 = 1043;
    pub const NAMEOID: u32 = 19;
    pub const NUMERICOID: u32 = 1700;
}

/// PostgreSQL built-in operator OIDs.
///
/// Well-known operator identifiers from PostgreSQL's pg_operator catalog.
pub mod operator_oids {
    // int4 operators
    pub const INT4EQ: u32 = 96;
    pub const INT4LT: u32 = 97;
    pub const INT4GT: u32 = 518;
    pub const INT4NE: u32 = 520;
    pub const INT4LE: u32 = 521;
    pub const INT4GE: u32 = 524;
    pub const INT4PL: u32 = 551;
    pub const INT4MI: u32 = 555;
    pub const INT4MUL: u32 = 514;
    pub const INT4DIV: u32 = 528;

    // int8 operators
    pub const INT8EQ: u32 = 410;
    pub const INT8LT: u32 = 412;
    pub const INT8GT: u32 = 413;
    pub const INT8NE: u32 = 411;
    pub const INT8LE: u32 = 414;
    pub const INT8GE: u32 = 415;

    // float8 operators
    pub const FLOAT8EQ: u32 = 670;
    pub const FLOAT8LT: u32 = 672;
    pub const FLOAT8GT: u32 = 674;
    pub const FLOAT8NE: u32 = 671;
    pub const FLOAT8LE: u32 = 673;
    pub const FLOAT8GE: u32 = 675;

    // text operators
    pub const TEXTEQ: u32 = 98;
    pub const TEXTNE: u32 = 531;
    pub const TEXTLT: u32 = 664;
    pub const TEXTLE: u32 = 665;
    pub const TEXTGT: u32 = 666;
    pub const TEXTGE: u32 = 667;
    pub const TEXTCAT: u32 = 654;

    // numeric operators
    pub const NUMERICEQ: u32 = 1752;
    pub const NUMERICLT: u32 = 1754;
    pub const NUMERICGT: u32 = 1756;
    pub const NUMERICNE: u32 = 1753;
    pub const NUMERICLE: u32 = 1755;
    pub const NUMERICGE: u32 = 1757;
}

/// PostgreSQL built-in aggregate function OIDs.
///
/// Well-known aggregate function identifiers from PostgreSQL's pg_proc catalog.
pub mod aggregate_oids {
    pub const COUNT_STAR: u32 = 2803;
    pub const COUNT_EXPR: u32 = 2147;

    // sum variants (int2/int4/int8/numeric)
    pub const SUM_INT2: u32 = 2108;
    pub const SUM_INT4: u32 = 2109;
    pub const SUM_INT8: u32 = 2110;
    pub const SUM_NUMERIC: u32 = 2111;

    // avg variants
    pub const AVG_INT2: u32 = 2100;
    pub const AVG_INT4: u32 = 2101;
    pub const AVG_INT8: u32 = 2102;
    pub const AVG_NUMERIC: u32 = 2103;
    pub const AVG_FLOAT4: u32 = 2104;
    pub const AVG_FLOAT8: u32 = 2105;
    pub const AVG_INTERVAL: u32 = 2106;

    // min variants
    pub const MIN_INT2: u32 = 2131;
    pub const MIN_INT4: u32 = 2132;
    pub const MIN_INT8: u32 = 2133;
    pub const MIN_NUMERIC: u32 = 2134;
    pub const MIN_FLOAT4: u32 = 2135;
    pub const MIN_FLOAT8: u32 = 2136;
    pub const MIN_DATE: u32 = 2137;
    pub const MIN_TIME: u32 = 2138;
    pub const MIN_TIMESTAMP: u32 = 2139;

    // max variants
    pub const MAX_INT2: u32 = 2115;
    pub const MAX_INT4: u32 = 2116;
    pub const MAX_INT8: u32 = 2117;
    pub const MAX_NUMERIC: u32 = 2118;
    pub const MAX_FLOAT4: u32 = 2119;
    pub const MAX_FLOAT8: u32 = 2120;
    pub const MAX_DATE: u32 = 2121;
    pub const MAX_TIME: u32 = 2122;
    pub const MAX_TIMESTAMP: u32 = 2123;
    pub const MAX_TEXT: u32 = 2126;

    // stddev / variance
    pub const STDDEV_POP: u32 = 2154;
    pub const STDDEV_SAMP: u32 = 2155;
    pub const VAR_POP: u32 = 2156;
    pub const VAR_SAMP: u32 = 2157;
    pub const VARIANCE: u32 = 2148;
    pub const VARIANCE_INT2: u32 = 2149;
    pub const VARIANCE_INT4: u32 = 2150;
    pub const VARIANCE_INT8: u32 = 2151;

    // string_agg
    pub const STRING_AGG: u32 = 3538;

    // array_agg
    pub const ARRAY_AGG: u32 = 2335;
}

/// Hardware-aware cost parameters.
///
/// These functions use the detected hardware profile to provide
/// optimal cost parameters for the actual system configuration.
pub mod hardware_aware {
    use crate::extension_state;

    use super::{cost_defaults, guc_tuning};

    /// Get optimal random_page_cost for detected storage.
    ///
    /// Returns SSD-optimized cost if fast storage detected (> 1.0 GB/s),
    /// otherwise returns HDD default.
    pub fn random_page_cost() -> f64 {
        let hw = extension_state::hardware_profile();

        // Storage bandwidth > 1 GB/s suggests SSD/NVMe
        // HDD typically maxes out around 0.2 GB/s
        if hw.storage_bandwidth_gbps > 1.0 {
            guc_tuning::RANDOM_PAGE_COST_SSD
        } else {
            cost_defaults::RANDOM_PAGE_COST
        }
    }

    /// Get optimal parallel worker count based on CPU cores.
    ///
    /// Returns min(cpu_cores / 2, 4) to avoid over-parallelization.
    pub fn max_parallel_workers() -> u32 {
        let hw = extension_state::hardware_profile();
        (hw.cpu_cores / 2).min(4).max(1)
    }

    /// Get recommended work_mem based on L3 cache and CPU cores.
    ///
    /// Uses L3 cache as a proxy for memory - systems with large L3
    /// typically have proportionally large RAM.
    /// Returns (L3_bytes / cpu_cores / 4) to allow ~4 queries per core.
    pub fn work_mem_mb() -> u64 {
        let hw = extension_state::hardware_profile();

        // L3 cache is a reasonable proxy: 32MB L3 suggests ~64GB RAM
        // Use conservative multiplier to avoid OOM
        let l3_mb = hw.l3_cache_bytes / (1024 * 1024);
        let estimated_ram_mb = l3_mb * 2048 / 32; // Scale from 32MB L3 = 64GB RAM

        // Divide by (cores * 4) to allow ~4 queries per core
        (estimated_ram_mb / (u64::from(hw.cpu_cores) * 4)).max(4) // Minimum 4MB
    }

    /// Check if system has sufficient resources for aggressive optimization.
    ///
    /// Returns true if >= 8 cores and large L3 cache (suggesting >= 16GB RAM).
    pub fn can_optimize_aggressively() -> bool {
        let hw = extension_state::hardware_profile();
        hw.cpu_cores >= 8 && hw.l3_cache_bytes >= 16 * 1024 * 1024 // >= 16MB L3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssd_cost_less_than_hdd() {
        assert!(guc_tuning::RANDOM_PAGE_COST_SSD < cost_defaults::RANDOM_PAGE_COST);
    }

    #[test]
    fn force_seqscan_cost_high() {
        assert!(guc_tuning::RANDOM_PAGE_COST_FORCE_SEQSCAN > cost_defaults::RANDOM_PAGE_COST);
    }

    #[test]
    fn cpu_tuple_cost_higher_than_operator() {
        assert!(cost_defaults::CPU_TUPLE_COST > cost_defaults::CPU_OPERATOR_COST);
    }

    #[test]
    fn guc_names_not_empty() {
        assert!(!guc_names::ENABLE_HASHJOIN.is_empty());
        assert!(!guc_names::RANDOM_PAGE_COST.is_empty());
    }
}
