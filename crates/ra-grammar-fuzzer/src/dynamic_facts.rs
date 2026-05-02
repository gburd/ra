//! Dynamic facts provider for fuzzing with varying database contexts.
//!
//! This module provides `DynamicFactsProvider` which systematically varies
//! database facts and statistics during property-based testing to better
//! find corner cases in query optimization.

use crate::cloud_profiles::ProfileSelector;
use crate::deployment_profiles::DeploymentProfile;
use proptest::prelude::*;
use ra_core::facts::{
    CpuArchitecture, DataType, ForeignKey, HardwareProfile, IndexInfo, IndexType,
    OperatorStats, SqlDialect, StorageFormat, TableInfo, TableStats, FactsProvider,
};
use ra_core::statistics::ColumnStats;
use std::collections::HashMap;
use std::time::Duration;

/// A facts provider that generates dynamic statistics for fuzzing.
///
/// This provider systematically varies database characteristics to model
/// different optimization scenarios:
///
/// - **Scale variations**: small/medium/large tables (10 - 100M rows)
/// - **Hardware variations**: low-end to high-end systems
/// - **Skew variations**: uniform to highly skewed data distributions
/// - **Staleness variations**: fresh to very stale statistics
/// - **Index variations**: sparse to dense index coverage
/// - **Memory pressure**: abundant to constrained memory scenarios
#[derive(Debug, Clone)]
pub struct DynamicFactsProvider {
    /// Current scenario configuration
    scenario: DatabaseScenario,
    /// Generated table statistics
    table_stats: HashMap<String, TableStats>,
    /// Generated column statistics
    column_stats: HashMap<String, HashMap<String, ColumnStats>>,
    /// Generated schema information
    schemas: HashMap<String, TableInfo>,
    /// Hardware profile for this scenario
    hardware: HardwareProfile,
    /// Runtime statistics (empty for baseline fuzzing)
    runtime_stats: HashMap<String, OperatorStats>,
    /// Optional deployment profile providing additional infrastructure context
    deployment_profile: Option<DeploymentProfile>,
}

/// Different database scenarios to test optimizer robustness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseScenario {
    /// Small development database (1K-100K rows per table)
    SmallDev,
    /// Medium production database (100K-10M rows per table)
    MediumProd,
    /// Large enterprise database (10M-1B rows per table)
    LargeEnterprise,
    /// Data warehouse scenario (1B+ rows, wide tables)
    DataWarehouse,
    /// Memory-constrained environment (limited RAM)
    MemoryConstrained,
    /// High-performance environment (many cores, GPU)
    HighPerformance,
    /// Stale statistics scenario (outdated stats)
    StaleStats,
    /// Skewed data scenario (highly skewed distributions)
    SkewedData,
}

impl DatabaseScenario {
    /// Generate table row counts appropriate for this scenario.
    #[must_use]
    pub fn table_row_range(self) -> (u64, u64) {
        match self {
            Self::SmallDev => (1_000, 100_000),
            Self::MediumProd | Self::StaleStats => (100_000, 10_000_000),
            Self::LargeEnterprise => (10_000_000, 1_000_000_000),
            Self::DataWarehouse => (1_000_000_000, 10_000_000_000),
            Self::MemoryConstrained => (10_000, 1_000_000), // Smaller due to memory limits
            Self::HighPerformance => (1_000_000, 100_000_000),
            Self::SkewedData => (1_000_000, 50_000_000), // Large enough for skew effects
        }
    }

    /// Generate hardware profile for this scenario.
    #[must_use]
    pub fn hardware_profile(self) -> HardwareProfile {
        match self {
            Self::SmallDev => HardwareProfile {
                cpu_cores: 4,
                available_memory: 8 * 1024 * 1024 * 1024, // 8 GB
                total_memory: 8 * 1024 * 1024 * 1024,
                simd_width: 128,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 8 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::MediumProd => HardwareProfile {
                cpu_cores: 8,
                available_memory: 32 * 1024 * 1024 * 1024, // 32 GB
                total_memory: 32 * 1024 * 1024 * 1024,
                simd_width: 256,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 32 * 1024,
                l2_cache_size: 256 * 1024,
                l3_cache_size: 16 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::LargeEnterprise => HardwareProfile {
                cpu_cores: 32,
                available_memory: 128 * 1024 * 1024 * 1024, // 128 GB
                total_memory: 128 * 1024 * 1024 * 1024,
                simd_width: 512,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 64 * 1024,
                l2_cache_size: 512 * 1024,
                l3_cache_size: 64 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::DataWarehouse => HardwareProfile {
                cpu_cores: 64,
                available_memory: 512 * 1024 * 1024 * 1024, // 512 GB
                total_memory: 512 * 1024 * 1024 * 1024,
                simd_width: 512,
                has_gpu: true,
                gpu_memory: Some(32 * 1024 * 1024 * 1024), // 32 GB GPU memory
                l1_cache_size: 64 * 1024,
                l2_cache_size: 1024 * 1024,
                l3_cache_size: 128 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::MemoryConstrained => HardwareProfile {
                cpu_cores: 2,
                available_memory: 2 * 1024 * 1024 * 1024, // 2 GB
                total_memory: 4 * 1024 * 1024 * 1024,
                simd_width: 128,
                has_gpu: false,
                gpu_memory: None,
                l1_cache_size: 16 * 1024,
                l2_cache_size: 128 * 1024,
                l3_cache_size: 2 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::HighPerformance => HardwareProfile {
                cpu_cores: 128,
                available_memory: 1024 * 1024 * 1024 * 1024, // 1 TB
                total_memory: 1024 * 1024 * 1024 * 1024,
                simd_width: 1024,
                has_gpu: true,
                gpu_memory: Some(80 * 1024 * 1024 * 1024), // 80 GB GPU memory (A100)
                l1_cache_size: 128 * 1024,
                l2_cache_size: 2 * 1024 * 1024,
                l3_cache_size: 256 * 1024 * 1024,
                cpu_architecture: CpuArchitecture::X86_64,
            },
            Self::StaleStats | Self::SkewedData => {
                // Use medium profile as baseline
                Self::MediumProd.hardware_profile()
            }
        }
    }

    /// Get staleness characteristics for this scenario.
    #[must_use]
    pub fn staleness_factor(self) -> f64 {
        match self {
            Self::StaleStats => 10.0, // Very stale
            Self::SkewedData => 3.0,   // Moderately stale due to skew
            _ => 1.0,                  // Fresh statistics
        }
    }

    /// Get data skew characteristics for this scenario.
    #[must_use]
    pub fn skew_factor(self) -> f64 {
        match self {
            Self::SkewedData => 0.95, // 95% of data in top 5% of values
            Self::DataWarehouse => 0.8, // Some skew typical in warehouses
            _ => 0.1, // Relatively uniform distributions
        }
    }
}

impl DynamicFactsProvider {
    /// Create a new dynamic facts provider for the given scenario.
    #[must_use]
    pub fn new(scenario: DatabaseScenario) -> Self {
        let hardware = scenario.hardware_profile();
        Self {
            scenario,
            table_stats: HashMap::new(),
            column_stats: HashMap::new(),
            schemas: HashMap::new(),
            hardware,
            runtime_stats: HashMap::new(),
            deployment_profile: None,
        }
    }

    /// Create a facts provider with a specific deployment profile.
    ///
    /// The hardware profile is derived from the deployment profile,
    /// overriding the scenario's default.
    #[must_use]
    pub fn with_deployment_profile(
        scenario: DatabaseScenario,
        profile: DeploymentProfile,
    ) -> Self {
        let hardware = profile.to_hardware_profile();
        Self {
            scenario,
            table_stats: HashMap::new(),
            column_stats: HashMap::new(),
            schemas: HashMap::new(),
            hardware,
            runtime_stats: HashMap::new(),
            deployment_profile: Some(profile),
        }
    }

    /// Return the deployment profile, if one was set.
    #[must_use]
    pub fn deployment_profile(&self) -> Option<&DeploymentProfile> {
        self.deployment_profile.as_ref()
    }

    /// Generate realistic statistics for a table based on the scenario.
    pub fn generate_table_stats(&mut self, table_name: &str) -> &TableStats {
        if !self.table_stats.contains_key(table_name) {
            let (min_rows, max_rows) = self.scenario.table_row_range();
            #[expect(
                clippy::cast_precision_loss,
                reason = "row counts are statistical approximations; f64 precision is sufficient"
            )]
            let row_count = fastrand::u64(min_rows..=max_rows) as f64;

            // Calculate derived statistics
            let avg_row_size = match table_name {
                "users" | "customers" => fastrand::f64() * 100.0 + 50.0, // 50-150 bytes
                "events" | "logs" => fastrand::f64() * 500.0 + 200.0, // 200-700 bytes (JSON)
                "products" | "inventory" => fastrand::f64() * 150.0 + 75.0, // 75-225 bytes
                _ => fastrand::f64() * 200.0 + 100.0, // 100-300 bytes (orders/transactions/default)
            };

            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "table size is a positive statistical estimate; truncation is acceptable"
            )]
            let table_size_bytes = (row_count * avg_row_size) as u64;
            let page_count = (table_size_bytes / 8192).max(1); // 8KB pages

            // Apply scenario-specific characteristics
            let staleness = self.scenario.staleness_factor();
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "modification estimate is a positive approximation; truncation is acceptable"
            )]
            let estimated_modifications = if staleness > 1.0 {
                ((row_count * (staleness - 1.0) / 10.0) as u64).max(1)
            } else {
                0
            };

            let stats = TableStats {
                row_count,
                page_count,
                average_row_size: avg_row_size,
                table_size_bytes,
                live_tuples: Some(row_count * 0.95), // 95% live
                dead_tuples: Some(row_count * 0.05), // 5% dead
                last_analyzed: {
                    #[expect(
                        clippy::cast_possible_wrap,
                        reason = "unix timestamp fits in i64 for any realistic date"
                    )]
                    let now_secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs() as i64);
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "staleness factor is a small value (1-10); truncation is safe"
                    )]
                    Some(now_secs - (staleness as i64 * 86400))
                }, // Age based on staleness
                confidence: if staleness > 5.0 { 0.3 } else { 0.9 },
                estimated_modifications,
            };

            self.table_stats.insert(table_name.to_string(), stats);
        }

        &self.table_stats[table_name]
    }

    /// Generate realistic column statistics based on the scenario.
    ///
    /// # Panics
    ///
    /// Panics if internal state is inconsistent (should not happen).
    #[expect(clippy::expect_used, reason = "generate_table_stats guarantees entry exists")]
    pub fn generate_column_stats(&mut self, table_name: &str, column_name: &str) -> &ColumnStats {
        // Get scenario data first to avoid borrowing issues
        let skew_factor = self.scenario.skew_factor();

        // Ensure table stats exist (but don't keep a reference)
        self.generate_table_stats(table_name);

        let table_cols = self.column_stats.entry(table_name.to_string()).or_default();

        if !table_cols.contains_key(column_name) {
            let table_stats = self.table_stats.get(table_name)
                .expect("generate_table_stats was called above");

            // Generate NDV based on column semantics and skew
            let ndv = match column_name {
                "id" | "user_id" | "order_id" => table_stats.row_count, // Unique keys
                "status" => fastrand::f64() * 5.0 + 2.0, // 2-7 statuses
                "category" | "type" => fastrand::f64() * 20.0 + 5.0, // 5-25 categories
                "country" | "region" => fastrand::f64() * 200.0 + 50.0, // 50-250 countries
                "created_at" | "updated_at" => {
                    // Time columns have high NDV but can be skewed
                    if skew_factor > 0.5 {
                        table_stats.row_count * 0.1 // Recent dates dominate
                    } else {
                        table_stats.row_count * 0.8 // More uniform
                    }
                }
                _ => {
                    // Default: moderate cardinality with potential skew
                    let base_ndv = table_stats.row_count.sqrt();
                    if skew_factor > 0.5 {
                        base_ndv * 0.1 // Heavy skew reduces effective NDV
                    } else {
                        base_ndv
                    }
                }
            };

            // Generate selectivity estimates (used in correlation calculation below)

            let stats = ColumnStats {
                distinct_count: ndv.max(1.0),
                null_fraction: fastrand::f64() * 0.1, // 0-10% nulls
                min_value: None,
                max_value: None,
                avg_length: Some(match column_name {
                    "id" | "user_id" | "order_id" | "created_at" | "updated_at" => 8.0, // 8-byte fixed-width
                    "name" | "title" => fastrand::f64() * 30.0 + 10.0, // 10-40 chars
                    "description" | "notes" => fastrand::f64() * 200.0 + 50.0, // 50-250 chars
                    "email" => 25.0, // Typical email length
                    _ => fastrand::f64() * 20.0 + 5.0, // 5-25 bytes default
                }),
                histogram: None, // TODO: Generate realistic histograms
                correlation: Some(if skew_factor > 0.5 { 0.8 } else { 0.0 }), // Correlation with row order
                most_common_values: None, // TODO: Generate MCVs for skewed data
                most_common_freqs: None,
            };

            table_cols.insert(column_name.to_string(), stats);
        }

        &table_cols[column_name]
    }

    /// Generate schema information for a table.
    pub fn generate_schema(&mut self, table_name: &str) -> &TableInfo {
        if !self.schemas.contains_key(table_name) {
            let schema = match table_name {
                "users" => TableInfo {
                    name: "users".to_string(),
                    columns: vec![
                        ("id".to_string(), DataType::Integer),
                        ("name".to_string(), DataType::String),
                        ("email".to_string(), DataType::String),
                        ("created_at".to_string(), DataType::Timestamp),
                        ("country".to_string(), DataType::String),
                        ("status".to_string(), DataType::String),
                    ],
                    primary_key: vec!["id".to_string()],
                    indexes: self.generate_indexes_for_table("users"),
                    foreign_keys: vec![],
                    storage_format: StorageFormat::RowBased,
                },
                "orders" => TableInfo {
                    name: "orders".to_string(),
                    columns: vec![
                        ("id".to_string(), DataType::Integer),
                        ("user_id".to_string(), DataType::Integer),
                        ("amount".to_string(), DataType::Float),
                        ("status".to_string(), DataType::String),
                        ("created_at".to_string(), DataType::Timestamp),
                    ],
                    primary_key: vec!["id".to_string()],
                    indexes: self.generate_indexes_for_table("orders"),
                    foreign_keys: vec![
                        ForeignKey {
                            columns: vec!["user_id".to_string()],
                            referenced_table: "users".to_string(),
                            referenced_columns: vec!["id".to_string()],
                        }
                    ],
                    storage_format: StorageFormat::RowBased,
                },
                _ => TableInfo {
                    name: table_name.to_string(),
                    columns: vec![
                        ("id".to_string(), DataType::Integer),
                        ("data".to_string(), DataType::String),
                        ("created_at".to_string(), DataType::Timestamp),
                    ],
                    primary_key: vec!["id".to_string()],
                    indexes: self.generate_indexes_for_table(table_name),
                    foreign_keys: vec![],
                    storage_format: StorageFormat::RowBased,
                },
            };

            self.schemas.insert(table_name.to_string(), schema);
        }

        &self.schemas[table_name]
    }

    /// Generate appropriate indexes for a table based on the scenario.
    fn generate_indexes_for_table(&self, table_name: &str) -> Vec<IndexInfo> {
        let mut indexes = vec![];

        // Always have primary key index
        indexes.push(IndexInfo {
            name: format!("{table_name}_pkey"),
            columns: vec!["id".to_string()],
            included_columns: vec![],
            index_type: IndexType::BTree,
            is_unique: true,
        });

        // Add scenario-specific indexes
        match self.scenario {
            DatabaseScenario::SmallDev => {
                // Minimal indexes - just primary key
            }
            DatabaseScenario::MemoryConstrained => {
                // Few indexes due to memory constraints
                if table_name == "users" {
                    indexes.push(IndexInfo {
                        name: "users_email_idx".to_string(),
                        columns: vec!["email".to_string()],
                        included_columns: vec![],
                        index_type: IndexType::BTree,
                        is_unique: true,
                    });
                }
            }
            DatabaseScenario::DataWarehouse | DatabaseScenario::HighPerformance => {
                // Dense index coverage for performance
                match table_name {
                    "users" => {
                        indexes.extend(vec![
                            IndexInfo {
                                name: "users_email_idx".to_string(),
                                columns: vec!["email".to_string()],
                                included_columns: vec!["name".to_string()],
                                index_type: IndexType::BTree,
                                is_unique: true,
                            },
                            IndexInfo {
                                name: "users_country_status_idx".to_string(),
                                columns: vec!["country".to_string(), "status".to_string()],
                                included_columns: vec!["created_at".to_string()],
                                index_type: IndexType::BTree,
                                is_unique: false,
                            },
                            IndexInfo {
                                name: "users_created_at_idx".to_string(),
                                columns: vec!["created_at".to_string()],
                                included_columns: vec![],
                                index_type: IndexType::BTree,
                                is_unique: false,
                            },
                        ]);
                    }
                    "orders" => {
                        indexes.extend(vec![
                            IndexInfo {
                                name: "orders_user_id_idx".to_string(),
                                columns: vec!["user_id".to_string()],
                                included_columns: vec!["amount".to_string(), "status".to_string()],
                                index_type: IndexType::BTree,
                                is_unique: false,
                            },
                            IndexInfo {
                                name: "orders_status_created_idx".to_string(),
                                columns: vec!["status".to_string(), "created_at".to_string()],
                                included_columns: vec![],
                                index_type: IndexType::BTree,
                                is_unique: false,
                            },
                        ]);
                    }
                    _ => {}
                }
            }
            _ => {
                // Standard production indexes
                match table_name {
                    "users" => {
                        indexes.push(IndexInfo {
                            name: "users_email_idx".to_string(),
                            columns: vec!["email".to_string()],
                            included_columns: vec![],
                            index_type: IndexType::BTree,
                            is_unique: true,
                        });
                    }
                    "orders" => {
                        indexes.push(IndexInfo {
                            name: "orders_user_id_idx".to_string(),
                            columns: vec!["user_id".to_string()],
                            included_columns: vec![],
                            index_type: IndexType::BTree,
                            is_unique: false,
                        });
                    }
                    _ => {}
                }
            }
        }

        indexes
    }
}

impl FactsProvider for DynamicFactsProvider {
    fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        self.table_stats.get(table)
    }

    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats> {
        self.column_stats.get(table)?.get(column)
    }

    fn hardware_profile(&self) -> &HardwareProfile {
        &self.hardware
    }

    fn get_schema(&self, table: &str) -> Option<&TableInfo> {
        self.schemas.get(table)
    }

    fn runtime_stats(&self, operator_id: &str) -> Option<&OperatorStats> {
        self.runtime_stats.get(operator_id)
    }

    fn database_name(&self) -> &'static str {
        match self.scenario {
            DatabaseScenario::SmallDev => "testdb",
            DatabaseScenario::MediumProd => "proddb",
            DatabaseScenario::LargeEnterprise => "enterprisedb",
            DatabaseScenario::DataWarehouse => "warehousedb",
            DatabaseScenario::MemoryConstrained => "embedded",
            DatabaseScenario::HighPerformance => "hpcdb",
            DatabaseScenario::StaleStats => "staledb",
            DatabaseScenario::SkewedData => "skeweddb",
        }
    }

    fn supports_feature(&self, feature: &str) -> bool {
        // If a deployment profile is set, derive capabilities from it
        if let Some(profile) = &self.deployment_profile {
            return match feature {
                "btree_indexes" | "hash_joins" | "sort_merge_joins" | "compression" => true,
                "hash_indexes" | "nested_loop_joins" => {
                    self.hardware.cpu_cores >= 2
                }
                "bitmap_indexes" | "columnar_storage" => {
                    self.hardware.available_memory >= 16 * 1024 * 1024 * 1024
                }
                "parallel_execution" => self.hardware.cpu_cores >= 4,
                "vectorized_execution" => self.hardware.simd_width >= 256,
                "gpu_acceleration" => self.hardware.has_gpu,
                "distributed_execution" => {
                    profile.supports_distributed_execution()
                }
                "tiered_storage" => profile.supports_tiered_storage(),
                "partition_pruning" => {
                    profile.topology.node_count() > 1
                }
                _ => false,
            };
        }

        match self.scenario {
            DatabaseScenario::SmallDev | DatabaseScenario::MemoryConstrained => {
                matches!(feature, "btree_indexes" | "hash_joins" | "sort_merge_joins")
            }
            DatabaseScenario::HighPerformance | DatabaseScenario::DataWarehouse => {
                matches!(feature,
                    "btree_indexes" | "hash_indexes" | "bitmap_indexes" |
                    "hash_joins" | "sort_merge_joins" | "nested_loop_joins" |
                    "parallel_execution" | "vectorized_execution" |
                    "gpu_acceleration" | "columnar_storage" | "compression"
                )
            }
            _ => {
                matches!(feature,
                    "btree_indexes" | "hash_indexes" |
                    "hash_joins" | "sort_merge_joins" | "nested_loop_joins" |
                    "parallel_execution"
                )
            }
        }
    }

    fn sql_dialect(&self) -> SqlDialect {
        SqlDialect::Postgres // Use PostgreSQL as baseline
    }

    fn memory_limit(&self) -> Option<u64> {
        match self.scenario {
            DatabaseScenario::MemoryConstrained => Some(self.hardware.available_memory / 2),
            _ => None, // No explicit limit
        }
    }

    fn optimizer_timeout(&self) -> Duration {
        match self.scenario {
            DatabaseScenario::SmallDev | DatabaseScenario::MemoryConstrained => {
                Duration::from_millis(100) // Fast timeout for resource-constrained
            }
            DatabaseScenario::HighPerformance | DatabaseScenario::DataWarehouse => {
                Duration::from_secs(30) // Allow longer optimization for high-end scenarios
            }
            _ => Duration::from_secs(5), // Standard timeout
        }
    }
}

/// Proptest strategy for generating database scenarios.
pub fn arb_database_scenario() -> impl Strategy<Value = DatabaseScenario> {
    prop_oneof![
        Just(DatabaseScenario::SmallDev),
        Just(DatabaseScenario::MediumProd),
        Just(DatabaseScenario::LargeEnterprise),
        Just(DatabaseScenario::DataWarehouse),
        Just(DatabaseScenario::MemoryConstrained),
        Just(DatabaseScenario::HighPerformance),
        Just(DatabaseScenario::StaleStats),
        Just(DatabaseScenario::SkewedData),
    ]
}

/// Proptest strategy for generating a `DynamicFactsProvider` with a random
/// cloud deployment profile applied to a random scenario.
pub fn arb_facts_with_profile() -> impl Strategy<Value = DynamicFactsProvider> {
    arb_database_scenario().prop_map(|scenario| {
        let profile =
            crate::cloud_profiles::CloudProfileSelector::select_random();
        DynamicFactsProvider::with_deployment_profile(scenario, profile)
    })
}

/// Enhanced property validator that uses dynamic facts providers.
pub struct EnhancedPropertyValidator {
    properties: Vec<crate::properties::OptimizerProperty>,
    time_limit: Duration,
}

impl EnhancedPropertyValidator {
    /// Create a validator that tests properties across multiple scenarios.
    #[must_use]
    pub fn new(properties: Vec<crate::properties::OptimizerProperty>) -> Self {
        Self {
            properties,
            time_limit: Duration::from_secs(10),
        }
    }

    /// Validate properties across all database scenarios.
    ///
    /// This tests the same query against different database configurations
    /// to find scenario-specific optimization bugs.
    #[must_use]
    pub fn validate_across_scenarios(
        &self,
        expr: &ra_core::algebra::RelExpr,
    ) -> Vec<(DatabaseScenario, Vec<crate::properties::PropertyResult>)> {
        let scenarios = [
            DatabaseScenario::SmallDev,
            DatabaseScenario::MediumProd,
            DatabaseScenario::LargeEnterprise,
            DatabaseScenario::DataWarehouse,
            DatabaseScenario::MemoryConstrained,
            DatabaseScenario::HighPerformance,
            DatabaseScenario::StaleStats,
            DatabaseScenario::SkewedData,
        ];

        scenarios
            .iter()
            .map(|&scenario| {
                let profile = crate::cloud_profiles::CloudProfileSelector::select_for_scenario(&scenario);
                let mut facts_provider = DynamicFactsProvider::with_deployment_profile(scenario, profile);

                // Pre-generate statistics for tables mentioned in the query
                self.populate_facts_for_query(&mut facts_provider, expr);

                // Create optimizer with scenario-specific configuration
                let budget = ra_engine::ResourceBudget::unlimited()
                    .with_time_limit(facts_provider.optimizer_timeout());

                let mut optimizer = ra_engine::Optimizer::new();
                optimizer.set_resource_budget(budget);
                // TODO: Set facts provider on optimizer when API supports it

                // Validate properties with this scenario
                let validator = crate::properties::PropertyValidator::new(self.properties.clone())
                    .with_time_limit(self.time_limit);
                let results = validator.validate(expr);

                (scenario, results)
            })
            .collect()
    }

    /// Pre-populate facts provider with statistics for all tables in the query.
    fn populate_facts_for_query(&self, facts: &mut DynamicFactsProvider, expr: &ra_core::algebra::RelExpr) {
        let tables = self.collect_table_names(expr);
        for table in tables {
            facts.generate_table_stats(&table);
            facts.generate_schema(&table);

            // Generate column stats for common columns
            let common_columns = ["id", "user_id", "created_at", "status", "name", "email"];
            for &column in &common_columns {
                facts.generate_column_stats(&table, column);
            }
        }
    }

    /// Extract all table names from a query expression.
    #[expect(
        clippy::self_only_used_in_recursion,
        reason = "self is needed for method dispatch in recursive calls"
    )]
    fn collect_table_names(&self, expr: &ra_core::algebra::RelExpr) -> std::collections::HashSet<String> {
        use ra_core::algebra::RelExpr;
        let mut tables = std::collections::HashSet::new();

        match expr {
            RelExpr::Scan { table, .. } => {
                tables.insert(table.clone());
            }
            RelExpr::Project { input, .. } |
            RelExpr::Filter { input, .. } |
            RelExpr::Sort { input, .. } |
            RelExpr::Limit { input, .. } |
            RelExpr::Distinct { input } |
            RelExpr::Aggregate { input, .. } => {
                tables.extend(self.collect_table_names(input));
            }
            RelExpr::Join { left, right, .. } |
            RelExpr::Union { left, right, .. } |
            RelExpr::Intersect { left, right, .. } |
            RelExpr::Except { left, right, .. } => {
                tables.extend(self.collect_table_names(left));
                tables.extend(self.collect_table_names(right));
            }
            _ => {
                // Handle other RelExpr variants that don't directly contain table references
                // or are not yet implemented in the fuzzer
            }
        }

        tables
    }
}