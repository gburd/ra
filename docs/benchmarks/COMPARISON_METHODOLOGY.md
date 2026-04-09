# Ra Benchmark Comparison Methodology

This document explains how Ra's performance comparison benchmarks are conducted, what metrics are measured, and how to interpret the results.

## Overview

Ra's benchmark suite compares its query optimization capabilities against native implementations in PostgreSQL, MySQL, SQLite, and DuckDB. The goal is to demonstrate how Ra's relational algebra rewrite rules can improve query execution across different database systems.

## Benchmark Structure

### Database Systems

We compare Ra against four major database systems:

1. **PostgreSQL** - Full-featured RDBMS with advanced optimizer
2. **MySQL** - Popular open-source RDBMS
3. **SQLite** - Embedded database with simpler optimizer
4. **DuckDB** - Analytics-focused embedded database

### Workload Types

Benchmarks are organized into six workload categories:

1. **Hybrid Search** - Combines full-text search with vector similarity
2. **Vector Search** - k-NN queries with distance functions
3. **Full-Text Search** - Text ranking and filtering
4. **Joins** - Multi-table join queries
5. **Aggregates** - GROUP BY and aggregate functions
6. **Analytics** - Window functions, CTEs, and complex analytics

Each workload contains multiple queries of varying complexity to test different optimization scenarios.

## Metrics Measured

### Primary Metrics

1. **Execution Time (ms)**
   - Native: Time for database to execute query
   - Ra: Time for Ra to optimize and generate execution plan
   - Note: This includes parsing and optimization overhead

2. **Speedup Ratio**
   - Formula: `speedup = native_time / ra_time`
   - Values > 1.0 indicate Ra is faster
   - Values < 1.0 indicate native implementation is faster

3. **Rows Scanned**
   - Estimated number of rows processed during execution
   - Lower is better (indicates more selective operations)
   - Demonstrates filter pushdown and index usage

4. **Query Complexity**
   - Subjective score (1-10) based on:
     - Number of tables joined
     - Presence of subqueries or CTEs
     - Number of predicates
     - Use of advanced features (window functions, etc.)

### Secondary Metrics

1. **Plan Quality**
   - Comparison of native EXPLAIN output vs Ra's optimized plan
   - Identification of optimization opportunities
   - Detection of inefficient plan choices

2. **Memory Usage** (future)
   - Peak memory consumption during optimization
   - Demonstrates Ra's efficiency

3. **Optimization Iterations** (future)
   - Number of rewrite rule applications
   - Shows optimization complexity

## Benchmark Execution

### Setup Phase

1. Create test databases in each system
2. Load schema definitions
3. Generate sample data (or use simulated statistics)
4. Warm up caches

### Execution Phase

For each query in each workload:

1. **Native Execution**
   - Parse SQL in target dialect
   - Execute EXPLAIN to get native plan
   - Measure execution time
   - Extract row scan estimates

2. **Ra Optimization**
   - Parse SQL to relational algebra
   - Apply rewrite rules
   - Generate optimized plan
   - Measure optimization time
   - Extract row scan estimates from optimized plan

3. **Comparison**
   - Calculate speedup ratio
   - Compare query plans
   - Identify optimization differences

### Measurement Methodology

**Timing**: We use high-resolution timers and run each query multiple times, taking the median to reduce noise from system variations.

**Simulated Execution**: Currently, benchmarks use simulated execution rather than actual database queries. This allows us to:
- Test against databases without requiring installation
- Focus on optimization quality rather than execution engine performance
- Ensure reproducible results

Future versions will support actual database execution for end-to-end performance validation.

## Statistical Analysis

### Speedup Categorization

- **Faster**: speedup > 1.1 (>10% improvement)
- **Slower**: speedup < 0.9 (<10% regression)
- **Similar**: 0.9 ≤ speedup ≤ 1.1 (within 10%)

### Aggregate Statistics

- **Average Speedup**: Mean across all queries
- **Median Speedup**: Middle value (less affected by outliers)
- **Max/Min Speedup**: Best and worst cases
- **Distribution**: Percentage of queries in each category

### Statistical Significance

For production benchmarks, we use:
- Multiple runs (n ≥ 5) to calculate standard deviation
- Confidence intervals (95%) to assess reliability
- T-tests to determine significance (p < 0.05)

## Interpreting Results

### When Ra Excels

Ra typically shows significant speedups in:

1. **Complex Join Queries**
   - Join reordering and commutativity
   - Multi-way join optimization
   - Example: 3+ table joins with selective predicates

2. **Hybrid Search Workloads**
   - Filter pushdown past expensive operations
   - Early materialization optimization
   - Combining multiple search modalities

3. **Queries with Redundant Operations**
   - Common subexpression elimination
   - Projection pruning
   - Dead code elimination

4. **Analytics Queries**
   - Window function optimization
   - CTE inlining and materialization decisions
   - Complex predicate rewriting

### When Native Wins

Native optimizers may outperform Ra in:

1. **Simple Queries**
   - Overhead of Ra's rewrite system
   - Native optimizers have fast paths for trivial queries

2. **Database-Specific Features**
   - Specialized index types (e.g., GiST, BRIN in PostgreSQL)
   - Vendor-specific optimizations
   - Hardware-accelerated operations

3. **Statistics-Driven Decisions**
   - Native has access to real cardinality information
   - Ra currently uses estimated or simulated statistics

### Neutral Cases

Some queries show similar performance because:

1. Both optimizers find the same optimal plan
2. Query is I/O bound rather than optimization bound
3. Differences are within measurement noise

## Reproducibility

### Running Benchmarks Locally

```bash
# Run all benchmarks
./scripts/run-all-benchmarks.sh

# Run specific database/workload
ra-cli benchmark --database postgresql --workload hybrid-search

# Generate reports
ra-cli benchmark --all --format html --output results.html
```

### Environment Considerations

Benchmark results can vary based on:

- **Hardware**: CPU speed, memory, storage type
- **Database Version**: Different optimizer implementations
- **Data Size**: Query complexity scales with data volume
- **System Load**: Background processes affect timing

For consistent results:
- Run on dedicated hardware
- Use same database versions
- Warm up caches before measurement
- Average multiple runs

### Data Collection

We provide:
- Raw benchmark data (JSON)
- Statistical summaries (Markdown)
- Interactive visualizations (HTML)
- Query plans for inspection

## Limitations and Caveats

### Current Limitations

1. **Simulated Execution**: Not measuring actual database performance yet
2. **Limited Statistics**: Using estimated cardinalities
3. **No Network Overhead**: Comparing local optimization only
4. **Synthetic Workloads**: Real-world queries may differ

### Future Improvements

1. **Actual Database Execution**: Connect to real databases
2. **Real Statistics**: Import table statistics from databases
3. **Cost Model Calibration**: Tune Ra's cost model per database
4. **Workload Diversity**: Add more real-world query patterns
5. **Parameterized Queries**: Test plan stability with parameters
6. **Concurrent Execution**: Multi-query optimization scenarios

### Known Issues

1. **Vector Operations**: Limited support in some databases
2. **Full-Text Search**: Syntax varies significantly across databases
3. **Database Extensions**: Not all features available everywhere
4. **Type Compatibility**: SQL type systems differ

## Best Practices

### For Benchmark Authors

1. Keep queries representative of real workloads
2. Document complexity rationale
3. Ensure queries are syntactically valid for target database
4. Include expected optimization opportunities in descriptions

### For Result Interpreters

1. Focus on trends, not individual results
2. Consider query complexity when evaluating speedups
3. Look at plan differences to understand optimizations
4. Account for measurement methodology limitations

### For System Tuners

1. Use benchmarks to identify optimization gaps
2. Validate new rewrite rules against benchmark suite
3. Detect performance regressions early
4. Compare across database systems to find patterns

## References

- **TPC Benchmarks**: Industry-standard database benchmarks
- **Join Order Benchmark**: Academic benchmark for join optimization
- **Database Internals** (Petrov): Explains optimizer architectures
- **Query Optimization Papers**: Volcano, Cascades, Orca optimizers

## Contact

For questions about benchmark methodology or to report issues:
- Open an issue on GitHub
- Check documentation at https://ra-optimizer.org
- Review source code in `crates/ra-cli/src/commands/benchmark.rs`
