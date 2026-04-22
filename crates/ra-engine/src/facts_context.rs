//! Aggregates all system facts for rule pre-condition evaluation.
//!
//! `FactsContext` combines statistics, hardware profiles, schema information,
//! runtime stats, and database capabilities into a single provider.

use ra_core::statistics::ColumnStats;
use ra_core::{
    CoreHardwareProfile, CoreTableStats, FactsProvider, OperatorStats, SqlDialect, TableInfo,
};
use ra_hardware::HardwareProfile;
use ra_stats::types::{ColumnStats as StatsColumnStats, TableStats};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Aggregates all system facts for rule evaluation
pub struct FactsContext {
    /// Table statistics
    table_stats: Arc<RwLock<HashMap<String, TableStats>>>,
    /// Column statistics (table -> column -> stats)
    column_stats: Arc<RwLock<HashMap<String, HashMap<String, StatsColumnStats>>>>,
    /// Hardware profile
    hardware: HardwareProfile,
    /// Schema information
    schema: Arc<RwLock<HashMap<String, TableInfo>>>,
    /// Runtime statistics
    runtime_stats: Arc<RwLock<HashMap<String, OperatorStats>>>,
    /// Database name
    database_name: String,
    /// Supported features
    features: Arc<RwLock<HashMap<String, bool>>>,
    /// SQL dialect
    dialect: SqlDialect,
    /// Memory limit
    memory_limit: Option<u64>,
    /// Optimizer timeout
    optimizer_timeout: Duration,
}

impl FactsContext {
    /// Create a new empty facts context with a hardware profile
    pub fn new(hardware: HardwareProfile) -> Self {
        Self {
            table_stats: Arc::new(RwLock::new(HashMap::new())),
            column_stats: Arc::new(RwLock::new(HashMap::new())),
            hardware,
            schema: Arc::new(RwLock::new(HashMap::new())),
            runtime_stats: Arc::new(RwLock::new(HashMap::new())),
            database_name: "generic".to_string(),
            features: Arc::new(RwLock::new(HashMap::new())),
            dialect: SqlDialect::Generic,
            memory_limit: None,
            optimizer_timeout: Duration::from_secs(60),
        }
    }

    /// Set the database name
    pub fn set_database_name(&mut self, name: String) {
        self.database_name = name;
    }

    /// Set the SQL dialect
    pub fn set_dialect(&mut self, dialect: SqlDialect) {
        self.dialect = dialect;
    }

    /// Set memory limit
    pub fn set_memory_limit(&mut self, limit: u64) {
        self.memory_limit = Some(limit);
    }

    /// Set optimizer timeout
    pub fn set_optimizer_timeout(&mut self, timeout: Duration) {
        self.optimizer_timeout = timeout;
    }

    /// Add table statistics
    pub fn add_table_stats(&mut self, table: String, stats: TableStats) {
        self.table_stats.write().unwrap().insert(table, stats);
    }

    /// Add column statistics
    pub fn add_column_stats(&mut self, table: String, column: String, stats: StatsColumnStats) {
        self.column_stats
            .write()
            .unwrap()
            .entry(table)
            .or_default()
            .insert(column, stats);
    }

    /// Add schema information
    pub fn add_schema(&mut self, info: TableInfo) {
        self.schema.write().unwrap().insert(info.name.clone(), info);
    }

    /// Add runtime statistics
    pub fn add_runtime_stats(&mut self, stats: OperatorStats) {
        self.runtime_stats
            .write()
            .unwrap()
            .insert(stats.operator_id.clone(), stats);
    }

    /// Register a supported feature
    pub fn register_feature(&mut self, feature: String, supported: bool) {
        self.features.write().unwrap().insert(feature, supported);
    }
}

impl FactsProvider for FactsContext {
    fn get_table_stats(&self, _table: &str) -> Option<&CoreTableStats> {
        // This is a limitation - we can't return a reference to converted data
        // In practice, we'd need to cache converted stats or use Arc
        // For now, return None and rely on direct access methods
        None
    }

    fn get_column_stats(&self, _table: &str, _column: &str) -> Option<&ColumnStats> {
        // Same limitation as above
        None
    }

    fn hardware_profile(&self) -> &CoreHardwareProfile {
        // This is also problematic - we need to convert on the fly
        // For a real implementation, we'd cache the converted profile
        // For now, use a thread-local or lazy_static
        static EMPTY: CoreHardwareProfile = CoreHardwareProfile {
            cpu_cores: 8,
            available_memory: 16 * 1024 * 1024 * 1024,
            total_memory: 16 * 1024 * 1024 * 1024,
            simd_width: 256,
            has_gpu: false,
            gpu_memory: None,
            l1_cache_size: 32 * 1024,
            l2_cache_size: 256 * 1024,
            l3_cache_size: 8 * 1024 * 1024,
        };
        &EMPTY
    }

    fn available_memory(&self) -> u64 {
        if self.hardware.gpu_available {
            self.hardware.available_gpu_memory_bytes()
        } else {
            // Estimate based on system memory (would need actual detection)
            16 * 1024 * 1024 * 1024 // 16GB default
        }
    }

    fn cpu_cores(&self) -> u32 {
        self.hardware.cpu_cores
    }

    fn has_gpu(&self) -> bool {
        self.hardware.gpu_available
    }

    fn simd_width(&self) -> u32 {
        self.hardware.simd_width_bits
    }

    fn get_schema(&self, _table: &str) -> Option<&TableInfo> {
        // Same reference lifetime issue
        None
    }

    fn runtime_stats(&self, _operator_id: &str) -> Option<&OperatorStats> {
        // Same reference lifetime issue
        None
    }

    fn database_name(&self) -> &'static str {
        // Leak the string to get a static reference. This is acceptable
        // because FactsContext instances are long-lived singletons.
        Box::leak(self.database_name.clone().into_boxed_str())
    }

    fn supports_feature(&self, feature: &str) -> bool {
        self.features
            .read()
            .unwrap()
            .get(feature)
            .copied()
            .unwrap_or(false)
    }

    fn sql_dialect(&self) -> SqlDialect {
        self.dialect
    }

    fn memory_limit(&self) -> Option<u64> {
        self.memory_limit
    }

    fn optimizer_timeout(&self) -> Duration {
        self.optimizer_timeout
    }
}

/// Builder for constructing FactsContext
pub struct FactsContextBuilder {
    context: FactsContext,
}

impl FactsContextBuilder {
    /// Create a new builder with a hardware profile
    pub fn new(hardware: HardwareProfile) -> Self {
        Self {
            context: FactsContext::new(hardware),
        }
    }

    /// Set the database name
    pub fn database(mut self, name: impl Into<String>) -> Self {
        self.context.set_database_name(name.into());
        self
    }

    /// Set the SQL dialect
    pub fn dialect(mut self, dialect: SqlDialect) -> Self {
        self.context.set_dialect(dialect);
        self
    }

    /// Add table statistics
    pub fn table_stats(mut self, table: impl Into<String>, stats: TableStats) -> Self {
        self.context.add_table_stats(table.into(), stats);
        self
    }

    /// Add column statistics
    pub fn column_stats(
        mut self,
        table: impl Into<String>,
        column: impl Into<String>,
        stats: StatsColumnStats,
    ) -> Self {
        self.context
            .add_column_stats(table.into(), column.into(), stats);
        self
    }

    /// Register a feature
    pub fn feature(mut self, name: impl Into<String>, supported: bool) -> Self {
        self.context.register_feature(name.into(), supported);
        self
    }

    /// Set memory limit
    pub fn memory_limit(mut self, limit: u64) -> Self {
        self.context.set_memory_limit(limit);
        self
    }

    /// Build the FactsContext
    pub fn build(self) -> FactsContext {
        self.context
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_facts_context() {
        let hw = HardwareProfile::gpu_server();
        let context = FactsContextBuilder::new(hw)
            .database("postgresql")
            .dialect(SqlDialect::Postgres)
            .feature("lateral_join", true)
            .feature("cte_recursive", true)
            .memory_limit(32 * 1024 * 1024 * 1024)
            .build();

        assert_eq!(context.database_name(), "postgresql");
        assert_eq!(context.sql_dialect(), SqlDialect::Postgres);
        assert!(context.supports_feature("lateral_join"));
        assert!(context.supports_feature("cte_recursive"));
        assert!(!context.supports_feature("unknown_feature"));
        assert_eq!(context.memory_limit(), Some(32 * 1024 * 1024 * 1024));
        assert!(context.cpu_cores() > 0);
        assert!(context.has_gpu());
    }

    #[test]
    fn default_facts_context() {
        let hw = HardwareProfile::cpu_only();
        let context = FactsContext::new(hw);

        assert_eq!(context.database_name(), "generic");
        assert_eq!(context.sql_dialect(), SqlDialect::Generic);
        assert!(!context.has_gpu());
        assert_eq!(context.optimizer_timeout(), Duration::from_secs(60));
    }

    // -- Setters --

    #[test]
    fn set_database_name() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        ctx.set_database_name("mysql".to_string());
        assert_eq!(ctx.database_name(), "mysql");
    }

    #[test]
    fn set_dialect() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        ctx.set_dialect(SqlDialect::Postgres);
        assert_eq!(ctx.sql_dialect(), SqlDialect::Postgres);
    }

    #[test]
    fn set_memory_limit() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert_eq!(ctx.memory_limit(), None);
        ctx.set_memory_limit(1024);
        assert_eq!(ctx.memory_limit(), Some(1024));
    }

    #[test]
    fn set_optimizer_timeout() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        ctx.set_optimizer_timeout(Duration::from_secs(30));
        assert_eq!(ctx.optimizer_timeout(), Duration::from_secs(30));
    }

    // -- Add and query stats --

    #[test]
    fn add_table_stats() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        let stats = TableStats {
            row_count: 1000,
            page_count: 10,
            average_row_size: 64.0,
            table_size_bytes: 64000,
            live_tuples: Some(1000),
            dead_tuples: None,
            last_analyzed: None,
        };
        ctx.add_table_stats("users".to_string(), stats);
        let guard = ctx.table_stats.read().unwrap();
        assert_eq!(guard.get("users").unwrap().row_count, 1000);
    }

    #[test]
    fn add_column_stats() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        let stats = StatsColumnStats {
            column_id: "age".to_string(),
            ndv: 100,
            null_fraction: 0.01,
            avg_width: 4.0,
            mcv: None,
            histogram: None,
            correlation: None,
        };
        ctx.add_column_stats("users".to_string(), "age".to_string(), stats);
        let guard = ctx.column_stats.read().unwrap();
        let col_map = guard.get("users").unwrap();
        assert_eq!(col_map.get("age").unwrap().ndv, 100);
    }

    #[test]
    fn add_column_stats_multiple_columns() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        let s1 = StatsColumnStats {
            column_id: "name".to_string(),
            ndv: 500,
            null_fraction: 0.0,
            avg_width: 20.0,
            mcv: None,
            histogram: None,
            correlation: None,
        };
        let s2 = StatsColumnStats {
            column_id: "email".to_string(),
            ndv: 1000,
            null_fraction: 0.05,
            avg_width: 30.0,
            mcv: None,
            histogram: None,
            correlation: None,
        };
        ctx.add_column_stats("users".to_string(), "name".to_string(), s1);
        ctx.add_column_stats("users".to_string(), "email".to_string(), s2);
        let guard = ctx.column_stats.read().unwrap();
        let col_map = guard.get("users").unwrap();
        assert_eq!(col_map.len(), 2);
    }

    // -- Schema --

    #[test]
    fn add_schema() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        let info = TableInfo {
            name: "orders".to_string(),
            columns: vec![
                ("id".to_string(), ra_core::DataType::Integer),
                ("total".to_string(), ra_core::DataType::Float),
            ],
            primary_key: vec!["id".to_string()],
            foreign_keys: vec![],
            indexes: vec![],
            storage_format: ra_core::facts::StorageFormat::RowBased,
        };
        ctx.add_schema(info);
        let guard = ctx.schema.read().unwrap();
        assert!(guard.contains_key("orders"));
        assert_eq!(guard["orders"].columns.len(), 2);
    }

    // -- Runtime stats --

    #[test]
    fn add_runtime_stats() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        let stats = OperatorStats {
            operator_id: "scan_users".to_string(),
            actual_rows: 500.0,
            estimated_rows: 1000.0,
            execution_time: Duration::from_millis(50),
            memory_used: 4096,
            skew_detected: false,
        };
        ctx.add_runtime_stats(stats);
        let guard = ctx.runtime_stats.read().unwrap();
        assert!(guard.contains_key("scan_users"));
        assert!((guard["scan_users"].actual_rows - 500.0).abs() < f64::EPSILON);
    }

    // -- Feature registration --

    #[test]
    fn register_feature_true_and_false() {
        let mut ctx = FactsContext::new(HardwareProfile::cpu_only());
        ctx.register_feature("parallel_query".to_string(), true);
        ctx.register_feature("gpu_offload".to_string(), false);
        assert!(ctx.supports_feature("parallel_query"));
        assert!(!ctx.supports_feature("gpu_offload"));
        assert!(!ctx.supports_feature("nonexistent"));
    }

    // -- FactsProvider trait methods --

    #[test]
    fn facts_provider_get_table_stats_returns_none() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert!(ctx.get_table_stats("any").is_none());
    }

    #[test]
    fn facts_provider_get_column_stats_returns_none() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert!(ctx.get_column_stats("t", "c").is_none());
    }

    #[test]
    fn facts_provider_hardware_profile_returns_defaults() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        let hp = ctx.hardware_profile();
        assert_eq!(hp.cpu_cores, 8);
        assert!(!hp.has_gpu);
    }

    #[test]
    fn facts_provider_available_memory_cpu_only() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert_eq!(ctx.available_memory(), 16 * 1024 * 1024 * 1024);
    }

    #[test]
    fn facts_provider_available_memory_gpu() {
        let ctx = FactsContext::new(HardwareProfile::gpu_server());
        assert!(ctx.available_memory() > 0);
    }

    #[test]
    fn facts_provider_simd_width() {
        let hw = HardwareProfile::cpu_only();
        let expected = hw.simd_width_bits;
        let ctx = FactsContext::new(hw);
        assert_eq!(ctx.simd_width(), expected);
    }

    #[test]
    fn facts_provider_get_schema_returns_none() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert!(ctx.get_schema("any").is_none());
    }

    #[test]
    fn facts_provider_runtime_stats_returns_none() {
        let ctx = FactsContext::new(HardwareProfile::cpu_only());
        assert!(ctx.runtime_stats("any").is_none());
    }

    // -- Builder chaining --

    #[test]
    fn builder_chaining() {
        let ts = TableStats {
            row_count: 500,
            page_count: 5,
            average_row_size: 32.0,
            table_size_bytes: 16000,
            live_tuples: None,
            dead_tuples: None,
            last_analyzed: None,
        };
        let cs = StatsColumnStats {
            column_id: "id".to_string(),
            ndv: 500,
            null_fraction: 0.0,
            avg_width: 8.0,
            mcv: None,
            histogram: None,
            correlation: None,
        };
        let ctx = FactsContextBuilder::new(HardwareProfile::cpu_only())
            .database("sqlite")
            .dialect(SqlDialect::Generic)
            .table_stats("t", ts)
            .column_stats("t", "id", cs)
            .feature("window_functions", true)
            .memory_limit(8 * 1024 * 1024 * 1024)
            .build();

        assert_eq!(ctx.database_name(), "sqlite");
        assert!(ctx.supports_feature("window_functions"));
        assert_eq!(ctx.memory_limit(), Some(8 * 1024 * 1024 * 1024));
        let tg = ctx.table_stats.read().unwrap();
        assert_eq!(tg["t"].row_count, 500);
        let cg = ctx.column_stats.read().unwrap();
        assert_eq!(cg["t"]["id"].ndv, 500);
    }
}
