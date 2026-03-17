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

## Support

- GitHub Issues: https://github.com/gregburd/ra/issues
- Discussions: https://github.com/gregburd/ra/discussions
- Documentation: https://docs.rs/ra-core
