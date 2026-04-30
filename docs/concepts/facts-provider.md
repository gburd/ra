# FactsProvider API Guide

## Overview

The `FactsProvider` trait provides a unified interface for accessing all system facts needed for rule pre-condition evaluation. It abstracts over different sources of facts (statistics, hardware, schema, runtime, database capabilities).

## Core Trait

```rust
pub trait FactsProvider: Send + Sync {
    // Statistics
    fn get_table_stats(&self, table: &str) -> Option<&TableStats>;
    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats>;

    // Hardware
    fn hardware_profile(&self) -> &HardwareProfile;
    fn available_memory(&self) -> u64;
    fn cpu_cores(&self) -> u32;
    fn has_gpu(&self) -> bool;
    fn simd_width(&self) -> u32;

    // Schema
    fn get_schema(&self, table: &str) -> Option<&TableInfo>;
    fn column_type(&self, table: &str, column: &str) -> Option<DataType>;
    fn has_index(&self, table: &str, columns: &[&str], index_type: Option<IndexType>) -> bool;
    fn has_primary_key(&self, table: &str) -> bool;
    fn foreign_keys(&self, table: &str) -> Vec<&ForeignKey>;

    // Runtime
    fn runtime_stats(&self, operator_id: &str) -> Option<&OperatorStats>;
    fn cardinality_error(&self, operator_id: &str) -> Option<f64>;

    // Database
    fn database_name(&self) -> &str;
    fn supports_feature(&self, feature: &str) -> bool;
    fn sql_dialect(&self) -> SqlDialect;

    // Resources
    fn memory_limit(&self) -> Option<u64>;
    fn optimizer_timeout(&self) -> Duration;
}
```

## Data Types

### TableStats

```rust
pub struct TableStats {
    pub row_count: f64,
    pub page_count: u64,
    pub average_row_size: f64,
    pub table_size_bytes: u64,
    pub live_tuples: Option<f64>,
    pub dead_tuples: Option<f64>,
    pub last_analyzed: Option<i64>,
    pub confidence: f64,  // 0.0 to 1.0
}
```

### ColumnStats

```rust
pub struct ColumnStats {
    pub distinct_count: f64,
    pub null_fraction: f64,
    pub avg_width: f64,
    pub most_common_values: Vec<String>,
    pub most_common_freqs: Vec<f64>,
    pub histogram_bounds: Vec<String>,
    pub confidence: f64,  // 0.0 to 1.0
}
```

### HardwareProfile

```rust
pub struct HardwareProfile {
    pub cpu_cores: u32,
    pub available_memory: u64,
    pub total_memory: u64,
    pub simd_width: u32,
    pub has_gpu: bool,
    pub gpu_memory: Option<u64>,
    pub l1_cache_size: u64,
    pub l2_cache_size: u64,
    pub l3_cache_size: u64,
}
```

### TableInfo

```rust
pub struct TableInfo {
    pub name: String,
    pub columns: Vec<(String, DataType)>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    pub indexes: Vec<IndexInfo>,
}
```

### OperatorStats

```rust
pub struct OperatorStats {
    pub operator_id: String,
    pub actual_rows: f64,
    pub estimated_rows: f64,
    pub execution_time: Duration,
    pub memory_used: u64,
    pub skew_detected: bool,
}
```

## Usage Examples

### Basic Statistics Query

```rust
use ra_core::{FactsProvider, EmptyFactsProvider};

let facts = EmptyFactsProvider::new();

if let Some(stats) = facts.get_table_stats("orders") {
    println!("Orders table: {} rows", stats.row_count);
    println!("Confidence: {:.2}", stats.confidence);
}
```

### Hardware-Aware Optimization

```rust
fn should_use_gpu_join(facts: &dyn FactsProvider, table: &str) -> bool {
    if !facts.has_gpu() {
        return false;
    }

    if let Some(stats) = facts.get_table_stats(table) {
        // GPU join beneficial for large tables
        stats.row_count > 1_000_000
    } else {
        false
    }
}
```

### Schema Introspection

```rust
fn has_suitable_index(
    facts: &dyn FactsProvider,
    table: &str,
    columns: &[&str],
) -> bool {
    facts.has_index(table, columns, Some(IndexType::BTree))
}

fn is_numeric_column(
    facts: &dyn FactsProvider,
    table: &str,
    column: &str,
) -> bool {
    facts.column_type(table, column)
        .map_or(false, |dt| dt.is_numeric())
}
```

### Runtime Feedback

```rust
fn cardinality_estimate_accurate(
    facts: &dyn FactsProvider,
    operator_id: &str,
) -> bool {
    facts.cardinality_error(operator_id)
        .map_or(true, |error| error < 2.0)  // Within 2x
}
```

## Implementing FactsProvider

### Simple In-Memory Provider

```rust
use std::collections::HashMap;
use ra_core::{FactsProvider, TableStats, HardwareProfile};

pub struct InMemoryFactsProvider {
    tables: HashMap<String, TableStats>,
    hardware: HardwareProfile,
}

impl InMemoryFactsProvider {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            hardware: HardwareProfile::detect(),
        }
    }

    pub fn add_table(&mut self, name: String, stats: TableStats) {
        self.tables.insert(name, stats);
    }
}

impl FactsProvider for InMemoryFactsProvider {
    fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        self.tables.get(table)
    }

    fn hardware_profile(&self) -> &HardwareProfile {
        &self.hardware
    }

    // ... implement other methods
}
```

### Database Adapter

```rust
use ra_core::FactsProvider;

pub struct PostgresFactsProvider {
    connection: tokio_postgres::Client,
    cache: FactsCache,
}

impl PostgresFactsProvider {
    pub async fn connect(url: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Connection error: {}", e);
            }
        });

        Ok(Self {
            connection: client,
            cache: FactsCache::new(),
        })
    }

    async fn gather_table_stats(&self, table: &str) -> Result<TableStats> {
        let row = self.connection.query_one(
            "SELECT reltuples, relpages FROM pg_class WHERE relname = $1",
            &[&table],
        ).await?;

        Ok(TableStats {
            row_count: row.get(0),
            page_count: row.get(1),
            // ...
        })
    }
}

impl FactsProvider for PostgresFactsProvider {
    fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        // Check cache first
        if let Some(stats) = self.cache.get_table_stats(table) {
            return Some(stats);
        }

        // Gather stats (requires async context)
        // In practice, you'd use a sync wrapper or pre-gather stats
        None
    }
}
```

## FactsContext Aggregator

The `FactsContext` struct aggregates multiple fact providers:

```rust
pub struct FactsContext {
    statistics: Box<dyn StatisticsProvider>,
    hardware: HardwareProfile,
    schema: SchemaInfo,
    runtime: RuntimeStatsCache,
    database: DatabaseCapabilities,
}

impl FactsProvider for FactsContext {
    fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        self.statistics.get_table_stats(table)
    }

    fn hardware_profile(&self) -> &HardwareProfile {
        &self.hardware
    }

    // Delegates to appropriate sub-providers
}
```

## Caching Strategy

Facts providers should implement caching to avoid repeated queries:

```rust
pub struct FactsCache {
    tables: HashMap<String, (TableStats, Instant)>,
    ttl: Duration,
}

impl FactsCache {
    pub fn new() -> Self {
        Self {
            tables: HashMap::new(),
            ttl: Duration::from_secs(300),  // 5 minutes
        }
    }

    pub fn get_table_stats(&self, table: &str) -> Option<&TableStats> {
        self.tables.get(table).and_then(|(stats, timestamp)| {
            if timestamp.elapsed() < self.ttl {
                Some(stats)
            } else {
                None  // Expired
            }
        })
    }
}
```

## Testing

### Mock Provider for Tests

```rust
use ra_core::{FactsProvider, TableStats, HardwareProfile};

pub struct MockFactsProvider {
    tables: HashMap<String, TableStats>,
}

impl MockFactsProvider {
    pub fn with_table(mut self, name: &str, row_count: f64) -> Self {
        self.tables.insert(
            name.to_string(),
            TableStats {
                row_count,
                confidence: 1.0,
                ..Default::default()
            },
        );
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_mock_facts() {
        let facts = MockFactsProvider::new()
            .with_table("orders", 1_000_000.0)
            .with_table("customers", 50_000.0);

        assert_eq!(
            facts.get_table_stats("orders").unwrap().row_count,
            1_000_000.0
        );
    }
}
```

## Best Practices

1. **Cache Aggressively:** Statistics are expensive to gather
2. **Set Confidence:** Always include confidence scores in statistics
3. **Handle Missing Data:** Pre-conditions should handle missing facts gracefully
4. **Lazy Loading:** Only gather facts when needed
5. **Thread Safety:** FactsProvider must be Send + Sync
6. **TTL for Cached Facts:** Statistics become stale as data changes

## Database-Specific Adapters

### PostgreSQL

Gather stats from `pg_stats` and `pg_class`:

```sql
SELECT reltuples, relpages FROM pg_class WHERE relname = 'table_name';
SELECT attname, n_distinct, null_frac FROM pg_stats WHERE tablename = 'table_name';
```

### MySQL

Gather stats from `INFORMATION_SCHEMA`:

```sql
SELECT table_rows, avg_row_length FROM INFORMATION_SCHEMA.TABLES WHERE table_name = 'table_name';
```

### DuckDB

Use `PRAGMA` commands:

```sql
PRAGMA table_info('table_name');
SELECT COUNT(*) FROM table_name;
```

## See Also

- [Pre-Condition System](PRECONDITIONS.md)
- [Database Integration Guide](DATABASE_INTEGRATION.md)
- [Hardware Detection](../crates/ra-hardware/README.md)
- [Statistics System](../crates/ra-stats-advanced/README.md)
