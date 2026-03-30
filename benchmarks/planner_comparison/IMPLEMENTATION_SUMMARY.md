# Planner Comparison Benchmark Implementation Summary

## Overview

The planner comparison benchmark harness has been implemented to compare Ra optimizer performance against PostgreSQL across 120 queries organized into 9 categories, as specified in BENCHMARK_PLAN.md.

## Files Created

### Directory Structure
```
benchmarks/planner_comparison/
├── README.md                    # Complete documentation
├── IMPLEMENTATION_SUMMARY.md    # This file
├── runner.rs                    # Main benchmark binary
├── collect_pg_plans.sh          # Script to gather PostgreSQL metrics
├── queries/                     # Query suite
│   ├── simple/                  # 10 single-table queries ✓
│   ├── basic_joins/             # 15 join queries (2-3 tables) ✓
│   ├── complex_joins/           # 3 complex join samples (need 17 more)
│   ├── aggregations/            # 3 aggregation samples (need 12 more)
│   ├── subqueries/              # 4 subquery samples (need 16 more)
│   ├── ctes/                    # 2 CTE samples (need 10 more)
│   ├── set_operations/          # 3 set operation samples (need 8 more)
│   ├── advanced/                # 1 advanced sample (need 8 more)
│   └── unsupported/             # 2 unsupported samples (need 6 more)
└── results/                     # Generated output (created on first run)
```

### Core Components

#### 1. Benchmark Runner (`runner.rs`)

**Features:**
- Loads SQL queries from all category directories
- Parses each query using `ra-parser::sql_to_relexpr`
- Optimizes using Ra's `Optimizer::optimize_with_facts()`
- Measures planning time with microsecond precision
- Tracks parser and optimizer success/failure
- Collects metrics: plan time, cost estimates, e-graph stats
- Generates JSON metrics and markdown report

**Key Structures:**
- `QueryMetrics` - Per-query metrics
- `CategorySummary` - Aggregated stats per category
- `BenchmarkReport` - Complete report with all data
- `OverallSummary` - Cross-category statistics

**Build and run:**
```bash
cargo build --release --bin planner_comparison_runner
cargo run --release --bin planner_comparison_runner
```

#### 2. Query Suite

**Current Status: 38/120 queries (32%)**

Completed categories:
- simple/ - 10/10 queries ✓
- basic_joins/ - 15/15 queries ✓

Partial categories:
- complex_joins/ - 3/20 queries (need 17 more)
- aggregations/ - 3/15 queries (need 12 more)
- subqueries/ - 4/20 queries (need 16 more)
- ctes/ - 2/12 queries (need 10 more)
- set_operations/ - 3/11 queries (need 8 more)
- advanced/ - 1/9 queries (need 8 more)
- unsupported/ - 2/8 queries (need 6 more)

**Query naming convention:**
- Numbered prefixes (01_, 02_, etc.)
- Descriptive names indicating feature tested
- Comment at top describing query purpose

#### 3. PostgreSQL Comparison Script (`collect_pg_plans.sh`)

**Features:**
- Connects to PostgreSQL database (default: tpch)
- Runs EXPLAIN (FORMAT JSON) on each query
- Extracts planning time and cost estimates
- Outputs JSON file with PostgreSQL metrics
- Requires: psql, jq

**Usage:**
```bash
cd benchmarks/planner_comparison
./collect_pg_plans.sh [database_name]
```

**Output:** `results/pg_plans.json`

#### 4. Documentation (`README.md`)

Complete documentation including:
- Directory structure explanation
- Query category descriptions
- Metrics collected
- Running instructions
- Performance targets from BENCHMARK_PLAN.md
- How to add new queries

## Integration with Cargo

Added to `crates/ra-engine/Cargo.toml`:
```toml
[dev-dependencies]
chrono = { workspace = true }

[[bin]]
name = "planner_comparison_runner"
path = "../../benchmarks/planner_comparison/runner.rs"
```

Added to workspace `Cargo.toml`:
```toml
chrono = "0.4"
```

## Metrics Collected

### Planning Efficiency
- `plan_time_us` - Ra optimization time (microseconds)
- `rules_applied` - Number of rewrite rules fired
- `egraph_nodes` - E-graph size
- `egraph_classes` - Equivalence classes
- `memory_allocated_bytes` - Memory usage

### Plan Quality
- `plan_cost_estimate` - Ra's cost estimate
- `pg_plan_cost` - PostgreSQL's cost (from collect_pg_plans.sh)
- `q_error` - Quality metric (estimated vs actual)

### Feature Coverage
- `parser_success` - Query parsed successfully
- `optimizer_success` - Query optimized successfully
- `error_message` - Error details if failed

## Output Format

### metrics.json
Complete benchmark results in JSON format:
- Timestamp and git commit
- Per-query metrics
- Category summaries
- Overall statistics

### COMPARISON_REPORT.md
Human-readable markdown report:
- Overall summary
- Results by category table
- Detailed per-query results
- Feature coverage statistics
- Failed queries with errors

## Next Steps

To complete the benchmark suite to 120 queries:

1. **Complex Joins (need 17 more):**
   - 5-6 table star joins
   - 7-8 table snowflake joins
   - Chain joins
   - Bushy join trees
   - Cross joins with filters

2. **Aggregations (need 12 more):**
   - More window functions (LAG, LEAD, DENSE_RANK, NTILE)
   - HAVING with complex predicates
   - Aggregate over aggregate
   - Multiple PARTITION BY
   - Aggregate with DISTINCT

3. **Subqueries (need 16 more):**
   - Correlated subqueries (5)
   - NOT IN subqueries (3)
   - Nested subqueries (3)
   - Subqueries in HAVING (2)
   - ANY/ALL subqueries (3)

4. **CTEs (need 10 more):**
   - Recursive CTEs (3)
   - Multiple CTEs with dependencies (3)
   - CTEs referenced 2+ times (2)
   - CTEs with aggregation (2)

5. **Set Operations (need 8 more):**
   - UNION ALL with filters (2)
   - INTERSECT ALL (2)
   - EXCEPT ALL (2)
   - Nested set operations (2)

6. **Advanced Features (need 8 more):**
   - More window functions (3)
   - VALUES with joins (2)
   - Complex CASE expressions (2)
   - COALESCE patterns (1)

7. **Unsupported Features (need 6 more):**
   - JSON_TABLE
   - PIVOT/UNPIVOT
   - CUBE/ROLLUP
   - TABLESAMPLE
   - FOR SYSTEM_TIME
   - FILTER clause on aggregates

## Performance Expectations

Based on existing TPC-H benchmarks:

| Category | Expected Median Time |
|----------|---------------------|
| Simple | 40-100ms |
| Basic joins | 500-1000ms |
| Complex joins | 1000-2000ms |
| Aggregations | 400-800ms |
| Subqueries | 700-1500ms |
| CTEs | 800-2000ms |
| Set operations | 600-1200ms |
| Advanced | 400-1000ms |
| Unsupported | Parse only or fail |

**Overall targets:**
- Parser success: >95%
- Optimizer success: >90% (of parsed)
- Median plan time: <1000ms
- P95 plan time: <2000ms

## References

- **BENCHMARK_PLAN.md** - Complete specification
- **benchmarks/tpch-ra-vs-pg.md** - Existing TPC-H results
- **benchmarks/job/benchmark_runner.rs** - Metrics framework
- **crates/ra-engine/benches/tpch_all22.rs** - TPC-H benchmark

## Testing

To verify the implementation:

```bash
# Build
cargo build --release --bin planner_comparison_runner

# Run on current query set (38 queries)
cargo run --release --bin planner_comparison_runner

# Check output
cat benchmarks/planner_comparison/results/COMPARISON_REPORT.md
jq '.overall_summary' benchmarks/planner_comparison/results/metrics.json

# Collect PostgreSQL comparison data (optional)
cd benchmarks/planner_comparison
./collect_pg_plans.sh tpch
```

## Known Limitations

1. **E-graph statistics not yet instrumented**
   - `egraph_nodes`, `egraph_classes`, `rules_applied` currently return 0
   - Requires adding instrumentation to `ra-engine::Optimizer`

2. **No actual execution**
   - Q-error cannot be computed without actual execution results
   - Comparison limited to planning metrics only

3. **PostgreSQL comparison requires setup**
   - Need PostgreSQL database with TPC-H schema
   - Need to run collect_pg_plans.sh separately

4. **Query suite incomplete**
   - Currently 38/120 queries (32%)
   - Sufficient for testing infrastructure
   - Need additional 82 queries for full coverage

## Future Enhancements

1. **Add e-graph instrumentation**
   - Track node count during optimization
   - Count rule applications
   - Measure memory consumption

2. **Add execution metrics**
   - Integrate with execution engine
   - Measure actual query runtime
   - Compute Q-error from actual vs estimated cost

3. **Enhanced PostgreSQL comparison**
   - Automatic database setup
   - Real-time EXPLAIN collection
   - Side-by-side plan visualization

4. **Continuous benchmarking**
   - Run on every commit (CI integration)
   - Track performance regressions
   - Baseline comparison

5. **Query generator**
   - Parameterized query templates
   - Automatic permutation generation
   - Coverage analysis
