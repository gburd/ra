# API Reference

This document describes the public API for using the relational algebra system as a library.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
ra-core = "0.1"
ra-parser = "0.1"
ra-engine = "0.1"
```

Basic usage:

```rust
use ra_core::{RelExpr, Expr};
use ra_engine::optimize;
use ra_parser::load_rules;

fn main() -> anyhow::Result<()> {
    // Load rules from directory
    let rules = load_rules("rules/")?;

    // Build a query
    let query = RelExpr::Filter {
        pred: Expr::gt(Expr::column("age"), Expr::const_i64(18)),
        input: Box::new(RelExpr::Scan {
            table: "users".to_string(),
        }),
    };

    // Optimize
    let optimized = optimize(query, &rules)?;

    println!("Optimized: {optimized:#?}");
    Ok(())
}
```

## ra-core

Core types and traits.

### RelExpr

Relational algebra expression tree.

```rust
pub enum RelExpr {
    Scan {
        table: String,
    },
    Filter {
        pred: Expr,
        input: Box<RelExpr>,
    },
    Project {
        cols: Vec<Expr>,
        input: Box<RelExpr>,
    },
    Join {
        join_type: JoinType,
        condition: Expr,
        left: Box<RelExpr>,
        right: Box<RelExpr>,
    },
    Aggregate {
        group_by: Vec<Expr>,
        aggregates: Vec<AggExpr>,
        input: Box<RelExpr>,
    },
    Sort {
        order_by: Vec<OrderExpr>,
        input: Box<RelExpr>,
    },
    Limit {
        limit: usize,
        offset: usize,
        input: Box<RelExpr>,
    },
    Union {
        all: bool,
        left: Box<RelExpr>,
        right: Box<RelExpr>,
    },
}
```

**Methods:**

```rust
impl RelExpr {
    /// Returns the output schema of this expression
    pub fn output_schema(&self) -> Schema;

    /// Returns all tables referenced by this expression
    pub fn referenced_tables(&self) -> HashSet<String>;

    /// Returns the estimated cost
    pub fn estimate_cost(&self, stats: &Statistics) -> Cost;

    /// Pretty-print the expression
    pub fn display(&self) -> String;
}
```

### Expr

Expression types for predicates and projections.

```rust
pub enum Expr {
    Column { name: String },
    Const { value: Value },
    BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    UnaryOp { op: UnaryOp, input: Box<Expr> },
    Function { name: String, args: Vec<Expr> },
    Case { conditions: Vec<(Expr, Expr)>, else_result: Box<Expr> },
    Cast { expr: Box<Expr>, target_type: DataType },
}
```

**Constructors:**

```rust
impl Expr {
    pub fn column(name: impl Into<String>) -> Self;
    pub fn const_i64(value: i64) -> Self;
    pub fn const_str(value: impl Into<String>) -> Self;
    pub fn const_bool(value: bool) -> Self;

    // Binary operators
    pub fn add(left: Expr, right: Expr) -> Self;
    pub fn sub(left: Expr, right: Expr) -> Self;
    pub fn mul(left: Expr, right: Expr) -> Self;
    pub fn div(left: Expr, right: Expr) -> Self;
    pub fn eq(left: Expr, right: Expr) -> Self;
    pub fn ne(left: Expr, right: Expr) -> Self;
    pub fn lt(left: Expr, right: Expr) -> Self;
    pub fn le(left: Expr, right: Expr) -> Self;
    pub fn gt(left: Expr, right: Expr) -> Self;
    pub fn ge(left: Expr, right: Expr) -> Self;
    pub fn and(left: Expr, right: Expr) -> Self;
    pub fn or(left: Expr, right: Expr) -> Self;

    // Unary operators
    pub fn not(expr: Expr) -> Self;
    pub fn is_null(expr: Expr) -> Self;
    pub fn is_not_null(expr: Expr) -> Self;

    // Methods
    pub fn referenced_columns(&self) -> HashSet<String>;
    pub fn is_deterministic(&self) -> bool;
    pub fn simplify(&self) -> Self;
}
```

### JoinType

```rust
pub enum JoinType {
    Inner,
    LeftOuter,
    RightOuter,
    FullOuter,
    Cross,
    Semi,      // Left semi-join (IN)
    Anti,      // Left anti-join (NOT IN)
}
```

### Rule Trait

```rust
pub trait Rule {
    /// Rule metadata
    fn metadata(&self) -> &RuleMetadata;

    /// Check if rule applies to this expression
    fn matches(&self, expr: &RelExpr) -> bool;

    /// Apply the rule
    fn apply(&self, expr: RelExpr) -> Option<RelExpr>;

    /// Estimate benefit of applying rule
    fn estimate_benefit(&self, expr: &RelExpr, stats: &Statistics) -> f64;
}
```

### Cost Model

```rust
pub trait CostModel {
    fn estimate_scan(&self, table: &str, stats: &Statistics) -> Cost;
    fn estimate_filter(&self, selectivity: f64, input_cost: Cost) -> Cost;
    fn estimate_join(&self, left: Cost, right: Cost, join_type: JoinType) -> Cost;
    fn estimate_aggregate(&self, input_cost: Cost, group_size: usize) -> Cost;
}

pub struct Cost {
    pub io_cost: f64,
    pub cpu_cost: f64,
    pub memory_cost: f64,
    pub total: f64,
}
```

## ra-parser

Parse `.rra` literate rule files.

### Loading Rules

```rust
use ra_parser::{load_rules, load_rule, RuleFile};

// Load all rules from directory
let rules: Vec<RuleFile> = load_rules("rules/")?;

// Load single rule
let rule: RuleFile = load_rule("rules/logical/predicate-pushdown/filter-through-join.rra")?;
```

### RuleFile

```rust
pub struct RuleFile {
    pub metadata: RuleMetadata,
    pub description: String,
    pub algebra_notation: Option<String>,
    pub implementation: Option<String>,
    pub test_cases: Vec<TestCase>,
    pub references: Vec<String>,
}

pub struct RuleMetadata {
    pub id: String,
    pub name: String,
    pub category: String,
    pub databases: Vec<String>,
    pub version: String,
    pub authors: Vec<String>,
    pub tags: Vec<String>,
}
```

## ra-engine

Query optimization engine.

### Basic Optimization

```rust
use ra_engine::{optimize, OptimizationConfig};

let config = OptimizationConfig {
    timeout_ms: 1000,
    max_iterations: 100,
    cost_model: Box::new(DefaultCostModel::new()),
};

let optimized = optimize(query, &rules, config)?;
```

### Advanced: E-graph API

```rust
use ra_engine::egraph::{EGraph, Rewrite};

// Build e-graph
let mut egraph = EGraph::new();
let root_id = egraph.add_expr(&query);

// Convert rules to rewrites
let rewrites: Vec<Rewrite> = rules.iter()
    .map(|r| r.to_rewrite())
    .collect();

// Run equality saturation
egraph.run_rewrites(&rewrites, 100);

// Extract best plan
let (cost, best_expr) = egraph.extract_best(root_id);
```

### Incremental Optimization

```rust
use ra_engine::differential::IncrementalOptimizer;

let mut optimizer = IncrementalOptimizer::new();

// Add initial rules
optimizer.add_rules(&rules);

// Optimize queries
let result1 = optimizer.optimize(query1)?;
let result2 = optimizer.optimize(query2)?;

// Update rules (only recomputes affected plans)
optimizer.update_rule(updated_rule)?;

// Re-optimize (incremental)
let result1_updated = optimizer.optimize(query1)?;
```

## ra-codegen

Generate executable code.

### Cranelift JIT

```rust
use ra_codegen::cranelift::CraneliftBackend;

let backend = CraneliftBackend::new();
let executable = backend.compile(&optimized_plan)?;

// Execute query
let results = executable.execute(&database)?;
```

### WASM

```rust
use ra_codegen::wasm::WasmBackend;

let backend = WasmBackend::new();
let wasm_module = backend.compile(&optimized_plan)?;

// Save to file
wasm_module.save("query.wasm")?;

// Or execute directly
let results = wasm_module.execute(&database)?;
```

## Examples

### Example 1: Optimize and Execute

```rust
use ra_core::RelExpr;
use ra_parser::load_rules;
use ra_engine::optimize;
use ra_codegen::cranelift::CraneliftBackend;

fn main() -> anyhow::Result<()> {
    // Load rules
    let rules = load_rules("rules/")?;

    // Parse SQL (using external parser)
    let query = parse_sql("SELECT * FROM users WHERE age > 18")?;

    // Optimize
    let optimized = optimize(query, &rules)?;

    // Generate code
    let backend = CraneliftBackend::new();
    let executable = backend.compile(&optimized)?;

    // Execute
    let results = executable.execute(&database)?;

    println!("Results: {results:?}");
    Ok(())
}
```

### Example 2: Custom Cost Model

```rust
use ra_core::{CostModel, Cost, Statistics};

struct CustomCostModel;

impl CostModel for CustomCostModel {
    fn estimate_scan(&self, table: &str, stats: &Statistics) -> Cost {
        // Custom logic here
        Cost {
            io_cost: stats.cardinality as f64 * 0.1,
            cpu_cost: stats.cardinality as f64 * 0.01,
            memory_cost: 0.0,
            total: stats.cardinality as f64 * 0.11,
        }
    }

    // Implement other methods...
}

// Use custom cost model
let config = OptimizationConfig {
    cost_model: Box::new(CustomCostModel),
    ..Default::default()
};

let optimized = optimize(query, &rules, config)?;
```

### Example 3: Rule Analysis

```rust
use ra_parser::load_rules;

fn analyze_rules() -> anyhow::Result<()> {
    let rules = load_rules("rules/")?;

    // Group by category
    let mut by_category = HashMap::new();
    for rule in &rules {
        by_category.entry(&rule.metadata.category)
            .or_insert_with(Vec::new)
            .push(rule);
    }

    // Print statistics
    for (category, rules) in by_category {
        println!("{category}: {} rules", rules.len());
    }

    // Find rules for specific database
    let postgres_rules: Vec<_> = rules.iter()
        .filter(|r| r.metadata.databases.contains(&"postgresql".to_string()))
        .collect();

    println!("PostgreSQL rules: {}", postgres_rules.len());
    Ok(())
}
```

## Error Handling

All functions return `Result` with appropriate error types:

```rust
use ra_core::error::RaError;

pub enum RaError {
    ParseError(String),
    ValidationError(String),
    OptimizationError(String),
    CodeGenError(String),
    IoError(std::io::Error),
}

impl From<std::io::Error> for RaError {
    fn from(err: std::io::Error) -> Self {
        RaError::IoError(err)
    }
}
```

## Feature Flags

Optional features in `Cargo.toml`:

```toml
[dependencies]
ra-engine = { version = "0.1", features = ["parallel", "differential"] }
```

Available features:
- `parallel` - Parallel optimization
- `differential` - Incremental maintenance
- `web` - Web API support
- `cli` - Command-line tools

## Performance Tips

1. **Reuse Optimizer**: Create once, use for multiple queries
2. **Set Timeouts**: Prevent long optimization times
3. **Cache Statistics**: Avoid recomputing table statistics
4. **Use Incremental Mode**: When optimizing similar queries
5. **Profile**: Use `cargo bench` to identify bottlenecks

## Thread Safety

All core types are `Send + Sync` and can be safely shared between threads.

```rust
use std::sync::Arc;
use rayon::prelude::*;

let rules = Arc::new(load_rules("rules/")?);

queries.par_iter().map(|query| {
    optimize(query.clone(), &rules)
}).collect::<Vec<_>>();
```

## Serialization

All types implement `serde::Serialize` and `serde::Deserialize`:

```rust
use serde_json;

// Serialize
let json = serde_json::to_string(&optimized)?;

// Deserialize
let expr: RelExpr = serde_json::from_str(&json)?;
```

## Documentation

Generate API documentation:

```bash
cargo doc --no-deps --all-features --open
```

## Statistics API

Ra's optimizer uses statistics to estimate cardinalities and choose optimal execution plans. This section describes how to provide statistics for tables, columns, indexes, and partitions.

### Table Statistics

Provide row counts, sizes, and storage information:

```rust
use ra_core::facts::TableStats;

// Create table statistics
let stats = TableStats {
    row_count: 1_000_000.0,          // Estimated rows
    page_count: 16_000,               // Pages/blocks on disk
    average_row_size: 128.0,          // Average row size in bytes
    table_size_bytes: 128_000_000,    // Total table size
    live_tuples: Some(990_000.0),     // Live rows (excluding deleted)
    dead_tuples: Some(10_000.0),      // Deleted but not vacuumed
    last_analyzed: Some(1710000000),  // Unix timestamp of last ANALYZE
    confidence: 0.95,                 // Confidence in statistics (0.0 to 1.0)
};

// Add to facts provider
let mut facts = build_facts_provider();
facts.add_table_stats("orders", stats);
```

**Best practices:**
- Update `row_count` regularly as data changes
- Set `confidence` lower for sampled or stale statistics
- Use `live_tuples` and `dead_tuples` to trigger VACUUM suggestions
- Track `last_analyzed` to know when statistics need refresh

### Column Statistics

Provide distinct counts, null fractions, and histograms:

```rust
use ra_core::statistics::{ColumnStats, Histogram, HistogramBucket, EquiDepthHistogram};

let col_stats = ColumnStats {
    distinct_count: 50_000.0,         // Number of distinct values (NDV)
    null_fraction: 0.05,              // Fraction of NULL values (0.0 to 1.0)
    min_value: Some("2020-01-01".into()),  // Minimum value as string
    max_value: Some("2024-12-31".into()),  // Maximum value as string
    avg_length: Some(24.0),           // Average length for variable-length columns
    histogram: Some(Histogram::EquiDepth(EquiDepthHistogram {
        buckets: vec![
            HistogramBucket {
                upper_bound: "2021-01-01".into(),
                row_count: 250_000.0,
                distinct_count: 12_500.0,
            },
            HistogramBucket {
                upper_bound: "2022-01-01".into(),
                row_count: 250_000.0,
                distinct_count: 12_500.0,
            },
            // More buckets...
        ],
        rows_per_bucket: 250_000.0,
    })),
};

facts.add_column_stats("orders", "order_date", col_stats);
```

**How Ra uses column statistics:**
- **Equality predicates:** Selectivity = `1 / distinct_count`
- **Range predicates:** Use histogram buckets to estimate rows in range
- **NULL filters:** Use `null_fraction` for `IS NULL` / `IS NOT NULL`
- **Join cardinality:** Use NDV to estimate join output size
- **String operations:** Use `avg_length` for memory estimation

**Histogram types:**
- `EquiWidth`: Fixed-width buckets (simpler, less accurate for skewed data)
- `EquiDepth`: Equal row count per bucket (better for skewed distributions)

### Index Definitions

Describe available indexes for access path selection:

```rust
use ra_core::facts::{IndexInfo, IndexType};

let idx = IndexInfo {
    name: "idx_orders_customer_date".into(),
    index_type: IndexType::BTree,
    columns: vec!["customer_id".into(), "order_date".into()],
    included_columns: vec!["amount".into(), "status".into()],  // Covering columns
    is_unique: false,
};

// Add index to table schema
let table_info = TableInfo {
    name: "orders".into(),
    columns: vec![
        ("order_id".into(), DataType::Integer),
        ("customer_id".into(), DataType::Integer),
        ("order_date".into(), DataType::Timestamp),
        ("amount".into(), DataType::Float),
        ("status".into(), DataType::String),
    ],
    primary_key: vec!["order_id".into()],
    foreign_keys: vec![],
    indexes: vec![idx],
};

facts.add_table_schema("orders", table_info);
```

**Index types:**
- `BTree`: General-purpose, supports range queries and ordering
- `Hash`: Fast equality lookups, no range queries
- `Bitmap`: Compact for low-cardinality columns
- `Gin`: Inverted index for JSON, arrays, full-text search
- `Gist`: Spatial, geometric, network address types
- `Brin`: Block-range for correlated data (timestamps, sequences)

**Covering indexes:**

Indexes with `included_columns` support index-only scans:

```rust
// Check if query can use covering index
if facts.has_covering_index("orders", &["customer_id", "order_date", "amount"]) {
    // Can satisfy query without heap access
}
```

### Partition Information

For partitioned tables, describe partition scheme and bounds:

```rust
use ra_core::distribution::{PartitionInfo, PartitionScheme, PartitionBound};

let partition_info = PartitionInfo {
    scheme: PartitionScheme::Range,
    key_columns: vec!["order_date".into()],
    partition_count: 12,
    partition_bounds: vec![
        PartitionBound::Range {
            lower: "2024-01-01".into(),
            upper: "2024-02-01".into(),
        },
        PartitionBound::Range {
            lower: "2024-02-01".into(),
            upper: "2024-03-01".into(),
        },
        // More partitions...
    ],
};

facts.add_partition_info("orders", partition_info);
```

**Partition schemes:**
- `Range`: Partition by ranges (timestamps, IDs)
- `Hash`: Distribute by hash of key columns
- `List`: Explicit value lists per partition

**Optimizer benefits:**
- **Partition pruning:** Skip irrelevant partitions based on predicates
- **Parallel scans:** Scan partitions in parallel
- **Partition-wise joins:** Join matching partitions directly

### Distribution Information

For distributed databases, describe data distribution across nodes:

```rust
use ra_core::distribution::{DistributionInfo, DistributionScheme};

let dist_info = DistributionInfo {
    scheme: DistributionScheme::Hash,
    distribution_key: vec!["customer_id".into()],
    node_count: 4,
    co_located_tables: vec!["customers".into(), "addresses".into()],
};

facts.add_distribution_info("orders", dist_info);
```

**Distribution schemes:**
- `SingleNode`: All data on one node (no network cost for joins)
- `Replicated`: Full copy on every node (broadcast joins)
- `Hash`: Sharded by hash of distribution key
- `Range`: Sharded by ranges

**Co-located tables:**

Tables with the same distribution key can be joined locally:

```rust
// Both tables distributed on customer_id - no data movement needed
SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id;
```

## Facts API

The Facts API provides access to statistics, schema, hardware, and runtime information.

### Core Trait

```rust
use ra_core::facts::{FactsProvider, TableStats, ColumnStats, IndexInfo, HardwareProfile};

pub trait FactsProvider: Send + Sync {
    // Statistics
    fn get_table_stats(&self, table: &str) -> Option<&TableStats>;
    fn get_column_stats(&self, table: &str, column: &str) -> Option<&ColumnStats>;

    // Schema
    fn get_schema(&self, table: &str) -> Option<&TableInfo>;
    fn column_type(&self, table: &str, column: &str) -> Option<DataType>;
    fn has_index(&self, table: &str, columns: &[&str], index_type: Option<IndexType>) -> bool;
    fn has_covering_index(&self, table: &str, needed_columns: &[&str]) -> bool;
    fn has_primary_key(&self, table: &str) -> bool;
    fn foreign_keys(&self, table: &str) -> Vec<&ForeignKey>;

    // Hardware
    fn hardware_profile(&self) -> &HardwareProfile;
    fn available_memory(&self) -> u64;
    fn cpu_cores(&self) -> u32;
    fn has_gpu(&self) -> bool;
    fn simd_width(&self) -> u32;

    // Runtime
    fn runtime_stats(&self, operator_id: &str) -> Option<&OperatorStats>;
    fn cardinality_error(&self, operator_id: &str) -> Option<f64>;

    // Database capabilities
    fn database_name(&self) -> &str;
    fn supports_feature(&self, feature: &str) -> bool;
    fn sql_dialect(&self) -> SqlDialect;

    // Resource limits
    fn memory_limit(&self) -> Option<u64>;
    fn optimizer_timeout(&self) -> Duration;
}
```

### Functional Dependencies

Declare functional dependencies for constraint-based optimization:

```rust
// order_id determines all other columns
facts.add_functional_dependency("orders", "order_id", vec!["customer_id", "order_date", "amount"]);

// Multi-column key
facts.add_functional_dependency("order_items", &["order_id", "item_id"], vec!["quantity", "price"]);
```

**Optimization uses:**
- Remove redundant GROUP BY columns
- Eliminate unnecessary DISTINCT operations
- Simplify join conditions

### Uniqueness Constraints

Declare unique columns for deduplication elimination:

```rust
// Single column uniqueness
facts.add_unique_constraint("users", vec!["email"]);

// Composite unique constraint
facts.add_unique_constraint("order_items", vec!["order_id", "item_id"]);
```

**Optimization uses:**
- Skip DISTINCT when grouping by unique key
- Use hash join instead of nested loop when joining on unique key
- Simplify EXISTS subqueries to semi-joins

### Foreign Key Relationships

Declare foreign keys for join elimination and reordering:

```rust
use ra_core::facts::ForeignKey;

let fk = ForeignKey {
    columns: vec!["customer_id".into()],
    referenced_table: "customers".into(),
    referenced_columns: vec!["id".into()],
};

facts.add_foreign_key("orders", fk);
```

**Optimization uses:**
- Join elimination: Remove joins to tables whose columns aren't used
- Join reordering: Prefer dimension-to-fact join order
- Referential integrity checks

### Check Constraints

For constraint-based optimization:

```rust
// Range constraint
facts.add_check_constraint("orders", "amount > 0");
facts.add_check_constraint("orders", "order_date >= '2020-01-01'");

// Enum constraint
facts.add_check_constraint("orders", "status IN ('pending', 'shipped', 'delivered')");
```

**Optimization uses:**
- Partition pruning based on predicates
- Range predicate simplification
- Dead code elimination

## Cost Model Parameters

Customize cost estimates for your hardware and workload:

```rust
use ra_core::cost::CostModelParams;

let params = CostModelParams {
    // Per-row costs
    cpu_tuple_cost: 0.01,             // CPU cost per tuple processed
    cpu_operator_cost: 0.0025,        // CPU cost per operator call

    // I/O costs
    seq_page_cost: 1.0,               // Sequential page fetch (baseline)
    random_page_cost: 4.0,            // Random page fetch (HDD)
                                      // Use 1.1 for SSD, 1.0 for in-memory

    // Join costs
    hash_join_build_cost: 0.01,       // CPU cost per tuple in hash table build
    hash_join_probe_cost: 0.005,      // CPU cost per tuple in hash probe
    merge_join_cost: 0.005,           // CPU cost per tuple in merge join
    nested_loop_cost: 0.001,          // CPU cost per inner tuple in nested loop

    // Aggregate costs
    hash_agg_build_cost: 0.015,       // CPU cost per tuple for hash aggregate
    sort_agg_cost: 0.01,              // CPU cost per tuple for sort-based aggregate

    // Sort costs
    sort_cost_factor: 1.5,            // Multiplier for sort operations

    // Memory parameters
    work_mem: 64 * 1024 * 1024,       // Memory per operation (64 MB)
    effective_cache_size: 4 * 1024 * 1024 * 1024,  // Available for caching (4 GB)

    // Network costs (for distributed databases)
    network_tuple_cost: 0.1,          // Cost per tuple sent over network
    network_startup_cost: 100.0,      // Fixed cost to initiate network transfer
};

optimizer.set_cost_model_params(params);
```

**Hardware-specific tuning:**

```rust
// HDD storage
let hdd_params = CostModelParams {
    seq_page_cost: 1.0,
    random_page_cost: 4.0,  // 4x penalty for random I/O
    ..Default::default()
};

// SSD storage
let ssd_params = CostModelParams {
    seq_page_cost: 1.0,
    random_page_cost: 1.1,  // Minimal penalty for random I/O
    ..Default::default()
};

// In-memory database
let memory_params = CostModelParams {
    seq_page_cost: 0.1,
    random_page_cost: 0.1,  // No I/O penalty
    cpu_tuple_cost: 0.001,  // CPU bound
    ..Default::default()
};
```

## Workload Profile API

Describe your workload characteristics for adaptive optimization:

```rust
use ra_core::workload::{WorkloadProfile, LatencySensitivity};

let profile = WorkloadProfile {
    // Read vs write ratio (0.0 = all writes, 1.0 = all reads)
    read_write_ratio: 0.95,

    // Latency requirements
    latency_sensitivity: LatencySensitivity::Interactive,  // <100ms target
    // Other options: Batch (seconds OK), Streaming (continuous)

    // Concurrency
    expected_concurrency: 100,        // Concurrent queries

    // Resource budgets
    memory_budget: 256 * 1024 * 1024, // 256 MB per query
    cpu_budget: 4,                    // CPU cores per query

    // Query characteristics
    typical_result_size: 1000,        // Typical rows returned
    uses_limit: true,                 // Queries often have LIMIT clauses

    // Data characteristics
    data_skew: 0.2,                   // Skew factor (0.0 = uniform, 1.0 = highly skewed)
};

optimizer.set_workload_profile(profile);
```

**Latency sensitivity impacts:**
- `Interactive`: Prefer plans with low startup cost, enable pipelining
- `Batch`: Prefer plans with low total cost, allow blocking operators
- `Streaming`: Maximize pipelining, avoid blocking operators

## Complete Examples

### E-commerce Database

```rust
use ra_core::{Optimizer, facts::*, statistics::*, workload::*};

fn setup_ecommerce_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();

    // Table statistics
    optimizer.add_table_stats("orders", TableStats {
        row_count: 5_000_000.0,
        page_count: 80_000,
        average_row_size: 128.0,
        table_size_bytes: 640_000_000,
        live_tuples: Some(4_950_000.0),
        dead_tuples: Some(50_000.0),
        last_analyzed: Some(current_timestamp()),
        confidence: 0.95,
    });

    optimizer.add_table_stats("customers", TableStats {
        row_count: 500_000.0,
        page_count: 8_000,
        average_row_size: 256.0,
        table_size_bytes: 128_000_000,
        live_tuples: Some(500_000.0),
        dead_tuples: Some(0.0),
        last_analyzed: Some(current_timestamp()),
        confidence: 0.98,
    });

    // Column statistics
    optimizer.add_column_stats("orders", "customer_id", ColumnStats {
        distinct_count: 450_000.0,      // Most customers have multiple orders
        null_fraction: 0.0,             // NOT NULL column
        min_value: Some("1".into()),
        max_value: Some("500000".into()),
        avg_length: None,               // Fixed-width integer
        histogram: None,                // Uniform distribution
    });

    optimizer.add_column_stats("orders", "order_date", ColumnStats {
        distinct_count: 1_095.0,        // 3 years of daily data
        null_fraction: 0.0,
        min_value: Some("2021-01-01".into()),
        max_value: Some("2023-12-31".into()),
        avg_length: None,
        histogram: Some(create_date_histogram("2021-01-01", "2023-12-31", 12)),
    });

    // Indexes
    let customer_idx = IndexInfo {
        name: "idx_orders_customer".into(),
        index_type: IndexType::BTree,
        columns: vec!["customer_id".into()],
        included_columns: vec!["order_date".into(), "total_amount".into()],
        is_unique: false,
    };

    let date_idx = IndexInfo {
        name: "idx_orders_date".into(),
        index_type: IndexType::Brin,    // BRIN for correlated timestamp data
        columns: vec!["order_date".into()],
        included_columns: vec![],
        is_unique: false,
    };

    optimizer.add_index("orders", customer_idx);
    optimizer.add_index("orders", date_idx);

    // Foreign keys
    optimizer.add_foreign_key("orders", ForeignKey {
        columns: vec!["customer_id".into()],
        referenced_table: "customers".into(),
        referenced_columns: vec!["id".into()],
    });

    // Workload profile
    optimizer.set_workload_profile(WorkloadProfile {
        read_write_ratio: 0.8,          // 80% reads, 20% writes
        latency_sensitivity: LatencySensitivity::Interactive,
        expected_concurrency: 50,
        memory_budget: 128 * 1024 * 1024,
        cpu_budget: 2,
        typical_result_size: 100,
        uses_limit: true,
        data_skew: 0.3,                 // Moderate skew (popular customers)
    });

    // Cost model (SSD storage)
    optimizer.set_cost_model_params(CostModelParams {
        seq_page_cost: 1.0,
        random_page_cost: 1.1,
        cpu_tuple_cost: 0.01,
        work_mem: 64 * 1024 * 1024,
        ..Default::default()
    });

    optimizer
}

// Optimize a query
let query = parse_sql("
    SELECT c.name, SUM(o.total_amount)
    FROM customers c
    JOIN orders o ON c.id = o.customer_id
    WHERE o.order_date >= '2023-01-01'
    GROUP BY c.id, c.name
    ORDER BY SUM(o.total_amount) DESC
    LIMIT 100
")?;

let plan = optimizer.optimize(query)?;
```

### Data Warehouse (OLAP)

```rust
fn setup_warehouse_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();

    // Large fact table with partitioning
    optimizer.add_table_stats("sales", TableStats {
        row_count: 1_000_000_000.0,     // 1 billion rows
        page_count: 8_000_000,
        average_row_size: 256.0,
        table_size_bytes: 256_000_000_000,
        live_tuples: Some(1_000_000_000.0),
        dead_tuples: Some(0.0),
        last_analyzed: Some(current_timestamp()),
        confidence: 0.90,               // Lower confidence for sampled stats
    });

    // Partition by month
    optimizer.add_partition_info("sales", PartitionInfo {
        scheme: PartitionScheme::Range,
        key_columns: vec!["sale_date".into()],
        partition_count: 36,            // 3 years monthly
        partition_bounds: create_monthly_partitions("2021-01", "2023-12"),
    });

    // Column statistics with high skew
    optimizer.add_column_stats("sales", "product_id", ColumnStats {
        distinct_count: 10_000.0,
        null_fraction: 0.0,
        min_value: Some("1".into()),
        max_value: Some("10000".into()),
        avg_length: None,
        histogram: Some(create_skewed_histogram()),  // Popular products
    });

    // Workload profile for OLAP
    optimizer.set_workload_profile(WorkloadProfile {
        read_write_ratio: 0.99,         // Almost all reads
        latency_sensitivity: LatencySensitivity::Batch,
        expected_concurrency: 10,
        memory_budget: 1024 * 1024 * 1024,  // 1 GB for large aggregations
        cpu_budget: 8,                  // Parallel execution
        typical_result_size: 1_000_000,
        uses_limit: false,
        data_skew: 0.8,                 // High skew typical in OLAP
    });

    // Cost model favoring parallelism
    optimizer.set_cost_model_params(CostModelParams {
        seq_page_cost: 1.0,
        random_page_cost: 1.2,
        parallel_setup_cost: 1000.0,
        parallel_tuple_cost: 0.1,
        work_mem: 256 * 1024 * 1024,    // Large work_mem for sorts/aggregates
        effective_cache_size: 32 * 1024 * 1024 * 1024,  // 32 GB cache
        ..Default::default()
    });

    optimizer
}
```

### Distributed Database

```rust
fn setup_distributed_optimizer() -> Optimizer {
    let mut optimizer = Optimizer::new();

    // Distributed tables
    optimizer.add_distribution_info("users", DistributionInfo {
        scheme: DistributionScheme::Hash,
        distribution_key: vec!["user_id".into()],
        node_count: 8,
        co_located_tables: vec!["sessions".into(), "orders".into()],
    });

    optimizer.add_distribution_info("products", DistributionInfo {
        scheme: DistributionScheme::Replicated,  // Small dimension, replicate
        distribution_key: vec![],
        node_count: 8,
        co_located_tables: vec![],
    });

    // Network cost parameters
    optimizer.set_cost_model_params(CostModelParams {
        network_tuple_cost: 0.1,        // Cost to send tuple over network
        network_startup_cost: 100.0,    // Fixed cost for network operation
        redistribute_cost: 1.0,         // Cost per tuple to redistribute
        broadcast_cost: 10.0,           // Cost per tuple to broadcast
        ..Default::default()
    });

    optimizer
}
```

## Best Practices

### Providing Statistics

1. **Update regularly:** Re-analyze tables after significant data changes (>10% rows)
2. **Use sampling for large tables:** Trade accuracy for performance
3. **Set confidence scores:** Lower confidence for sampled or stale stats
4. **Collect histograms for filtered columns:** Columns in WHERE clauses benefit most
5. **Track correlation:** For BRIN indexes and incremental sort

### Extracting from Production

**PostgreSQL:**
```sql
-- Table statistics
SELECT
    schemaname, tablename,
    n_live_tup as row_count,
    pg_total_relation_size(schemaname||'.'||tablename) as table_size_bytes
FROM pg_stat_user_tables;

-- Column statistics
SELECT
    tablename, attname,
    n_distinct as distinct_count,
    null_frac as null_fraction,
    avg_width
FROM pg_stats;
```

**MySQL:**
```sql
-- Table statistics
SELECT
    table_name,
    table_rows as row_count,
    data_length as table_size_bytes,
    avg_row_length as average_row_size
FROM information_schema.tables
WHERE table_schema = 'your_database';
```

### Sampling Strategies

For large tables, sample to estimate statistics:

```rust
// Sample 10% of rows
let sample_size = (total_rows * 0.1) as usize;
let sampled_stats = compute_statistics_sample(table, sample_size);

// Set confidence based on sample size
sampled_stats.confidence = (sample_size as f64 / total_rows).min(1.0);
```

### Handling Stale Statistics

```rust
fn should_reanalyze(stats: &TableStats) -> bool {
    let age_seconds = current_timestamp() - stats.last_analyzed.unwrap_or(0);
    let age_days = age_seconds / 86400;

    // Reanalyze if >7 days old or confidence <0.7
    age_days > 7 || stats.confidence < 0.7
}
```

### Testing with Different Statistics

Test optimizer behavior with varying statistics:

```rust
#[test]
fn test_plan_selection_by_cardinality() {
    let mut optimizer = Optimizer::new();

    // Small table - prefer nested loop
    optimizer.add_table_stats("orders", TableStats {
        row_count: 100.0,
        confidence: 1.0,
        ..Default::default()
    });
    let plan_small = optimizer.optimize(query.clone())?;
    assert!(matches!(plan_small.root(), Join::NestedLoop { .. }));

    // Large table - prefer hash join
    optimizer.add_table_stats("orders", TableStats {
        row_count: 1_000_000.0,
        confidence: 1.0,
        ..Default::default()
    });
    let plan_large = optimizer.optimize(query.clone())?;
    assert!(matches!(plan_large.root(), Join::Hash { .. }));
}
```

## Cross-References

- [Query Pattern Encyclopedia](./query-patterns.md) - Common query patterns and optimal plans
- [Rule Documentation](./rules/README.md) - Which rules use which statistics
- [Cost Model](./cost-model.md) - How costs are computed from statistics
- [Facts Provider Guide](./concepts/facts-provider.md) - Implementing custom fact providers

## Platform-Specific APIs

### PostgreSQL RUM Index Detection

```rust
use ra_engine::rum_index::{RumQueryType, RumOpclass, detect_rum};

// Detect if RUM is available
let has_rum = detect_rum(&connection)?;

// Classify query for RUM optimization
let query_type = RumQueryType::classify(&query)?;
if query_type.benefits_from_rum() {
    println!("RUM index recommended for {} query", query_type.label());
}

// Estimate RUM index cost
let cost = rum_index_cost(
    RumQueryType::RankedRetrieval,
    RumOpclass::TsvectorOps,
    0.01,  // selectivity
    Some(10),  // limit
);
```

### Citus Distributed Optimization

```rust
use ra_engine::citus_optimizer::{
    CitusMetadata, DistributedTableInfo, detect_citus
};

// Detect Citus and load metadata
let metadata = detect_citus(&connection)?;

// Check if tables are co-located
let tables = vec!["orders", "shipments"];
if metadata.are_colocated(&tables) {
    // Apply co-located join optimization
    let optimized = optimize_colocated_join(query, &metadata)?;
}

// Estimate columnar scan cost
let cost = columnar_scan_cost(
    10,   // total columns
    3,    // projected columns
    3.0,  // compression ratio
    1_000_000,  // row count
);
```

### DocumentDB BSON Optimization

```rust
use ra_engine::documentdb_optimizer::{
    BsonOperator, detect_documentdb, bson_selectivity
};

// Detect DocumentDB extension
let has_documentdb = detect_documentdb(&connection)?;

// Get operator-specific selectivity
let op = BsonOperator::from_pg_operator("@=")?;
let selectivity = op.default_selectivity();  // 0.005 instead of 0.01

// Recommend GIN index for multi-path query
let recommendations = recommend_bson_indexes(&query, &metadata)?;
```

### Oracle JSON Duality

```rust
use ra_engine::oracle_json_duality::{
    DualityView, detect_duality_views
};

// Load duality view definitions
let views = detect_duality_views(&connection)?;

// Choose access path (document vs relational)
let access_path = choose_duality_access_path(
    &view,
    &predicate,
    &metadata,
)?;

match access_path {
    AccessPath::DocumentFetch => {
        // Single document fetch by primary key
    }
    AccessPath::RelationalDecomposition => {
        // Join across base tables
    }
}
```

### XPath/XQuery Optimization

```rust
use ra_engine::xml_optimizer::{
    XPathAxis, parse_xpath, xpath_cost
};

// Parse XPath expression
let xpath = parse_xpath("/order/customer[@id='12345']")?;

// Estimate traversal cost
let cost = xpath.axes().iter()
    .map(|axis| axis.navigation_cost())
    .sum();

// Check if path index can optimize
if xpath.can_use_path_index() {
    // Apply path index optimization
}
```

### Document Algebra

```rust
use ra_core::document_algebra::{
    DocPredicate, DocumentScan, DocumentFilter, DocumentUnwind
};

// Build document query
let query = DocumentFilter {
    predicate: DocPredicate::Eq(
        FieldPath::new("status"),
        DocValue::String("active".to_string()),
    ),
    input: Box::new(DocumentScan {
        collection: "orders".to_string(),
    }),
};

// Check TOAST cost
let field_size = estimate_field_size(&collection, &field_path);
if field_size > TOAST_THRESHOLD {
    // Adjust cost for TOAST fetch
    let toast_cost = toast_aware_scan_cost(field_size, TOAST_THRESHOLD);
}
```

## Support

- Issues: https://codeberg.org/gregburd/ra/issues
- Discussions: https://codeberg.org/gregburd/ra/issues
- Documentation: https://docs.rs/ra-core
- Platform Optimizations: [Platform-Specific Optimizations](features/platform-optimizations.md)
