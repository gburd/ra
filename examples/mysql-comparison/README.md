# MySQL Performance Comparison Examples

This directory contains benchmark examples comparing MySQL native execution vs Ra-optimized query execution.

## Prerequisites

1. MySQL 5.7+ or MySQL 8.0+ installed and running
2. Create a test database:
```sql
CREATE DATABASE benchmark;
```

3. Set environment variable:
```bash
export TEST_MYSQL_URL="mysql://root:password@localhost:3306/benchmark"
```

## Examples

### benchmark_fulltext.rs
Compares MySQL FULLTEXT search performance (MATCH...AGAINST) between native and Ra-optimized execution.

**Tests:**
- Simple FULLTEXT search
- Boolean mode search with operators (+, -)
- Relevance scoring and ranking
- Combined FULLTEXT with filtering
- FULLTEXT with JOINs

**Run:**
```bash
cargo run --example benchmark_fulltext --features mysql
```

### benchmark_joins.rs
Compares JOIN query performance across different JOIN strategies.

**Tests:**
- INNER JOIN with filtering
- LEFT JOIN with aggregation
- Multiple JOINs (3+ tables)
- Self JOINs
- JOIN with GROUP BY
- JOIN with subqueries

**Run:**
```bash
cargo run --example benchmark_joins --features mysql
```

### benchmark_aggregates.rs
Compares aggregate query performance (GROUP BY, COUNT, SUM, etc.).

**Tests:**
- Simple GROUP BY with COUNT
- Multiple aggregates (SUM, AVG, MIN, MAX)
- GROUP BY with HAVING clause
- Multi-column GROUP BY
- Window functions (MySQL 8.0+)
- COUNT DISTINCT operations

**Run:**
```bash
cargo run --example benchmark_aggregates --features mysql
```

## Output

Each benchmark generates:
1. Console output with a formatted comparison table
2. JSON report file (e.g., `mysql_fulltext_benchmark.json`)

The JSON report contains detailed metrics including:
- Execution time (milliseconds)
- Rows returned
- Rows scanned (from EXPLAIN)
- Index usage
- Cost estimates
- Speedup ratio
- Improvement percentage

## Interpreting Results

- **Speedup > 1.0**: Ra optimization improved performance
- **Speedup < 1.0**: Native MySQL was faster (potential regression)
- **Improvement %**: Percentage improvement (positive = better, negative = regression)

Example output:
```
| Query                                      | Native (ms) | Ra (ms) | Speedup | Improvement |
|--------------------------------------------|-------------|---------|---------|-------------|
| SELECT * FROM articles WHERE MATCH...     | 45          | 22      | 2.05x   | 51.1%       |
```

## Testing

Run all MySQL comparison tests:
```bash
cargo test -p ra-adapters --features mysql mysql_comparison
```

Note: Tests are marked as `#[ignore]` and require a running MySQL instance.

## Architecture

The MySQL adapter (`crates/ra-adapters/src/mysql.rs`) provides:
- Connection pooling via r2d2
- Native query execution with timing
- Ra-optimized query execution
- EXPLAIN FORMAT=JSON support
- FULLTEXT index detection
- Handler statistics tracking

The comparison module (`crates/ra-adapters/src/comparison.rs`) provides:
- Generic comparison framework
- MySQL-specific EXPLAIN parsing
- Performance metrics collection
- Report generation (JSON and Markdown)

## Extending

To add new benchmarks:
1. Create a new `.rs` file in this directory
2. Follow the pattern from existing examples
3. Add the example to `Cargo.toml`
4. Document expected results and insights
