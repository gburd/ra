# Planner Comparison Benchmark

This benchmark harness compares Ra optimizer performance against PostgreSQL planner across 120 queries organized into 9 categories.

## Directory Structure

```
benchmarks/planner_comparison/
├── README.md                    # This file
├── runner.rs                    # Benchmark runner binary
├── queries/                     # Query suite (120 queries)
│   ├── simple/                  # 10 single-table queries
│   ├── basic_joins/             # 15 join queries (2-3 tables)
│   ├── complex_joins/           # 20 complex join queries (4+ tables)
│   ├── aggregations/            # 15 aggregation queries
│   ├── subqueries/              # 20 subquery patterns
│   ├── ctes/                    # 12 CTE queries
│   ├── set_operations/          # 11 UNION/INTERSECT/EXCEPT
│   ├── advanced/                # 9 advanced SQL features
│   └── unsupported/             # 8 unsupported features
└── results/                     # Generated benchmark results
    ├── metrics.json             # Raw metrics (JSON)
    └── COMPARISON_REPORT.md     # Human-readable report

## Query Categories

### 1. Simple Queries (10 queries)
Single-table scans with filters, aggregations, and sorting.

**Features tested:**
- Simple WHERE predicates
- Basic aggregates (COUNT, SUM, AVG, MIN, MAX)
- GROUP BY and HAVING
- ORDER BY and LIMIT/OFFSET
- DISTINCT

**Expected optimizations:**
- Predicate simplification
- Expression constant folding
- Aggregate pushdown to scan
- Parquet column pruning

### 2. Basic Joins (15 queries)
Simple joins with 2-3 tables.

**Features tested:**
- INNER JOIN, LEFT/RIGHT/FULL OUTER JOIN
- JOIN ON predicates
- WHERE filters
- Foreign key joins
- Self joins

**Expected optimizations:**
- Join order selection
- Predicate pushdown into scans
- Join predicate inference
- Foreign key recognition

### 3. Complex Joins (20 queries)
Multi-table joins with 4+ tables.

**Features tested:**
- Star schema joins
- Snowflake schema joins
- Chain joins
- Multi-way joins

**Expected optimizations:**
- Join reordering (all permutations)
- Semi-join reduction
- Bushy vs left-deep vs right-deep trees
- Runtime filter candidates

### 4. Aggregations (15 queries)
Queries with complex aggregation patterns.

**Features tested:**
- GROUP BY with multiple columns
- HAVING clause
- Window functions (ROW_NUMBER, RANK, DENSE_RANK)
- Multiple aggregates

**Expected optimizations:**
- Aggregate pushdown below joins
- Two-phase aggregation
- Window function sharing
- HAVING filter pushdown

### 5. Subqueries (20 queries)
Nested SELECT statements.

**Features tested:**
- Scalar subqueries in SELECT list
- EXISTS/NOT EXISTS
- IN/NOT IN subqueries
- Correlated subqueries

**Expected optimizations:**
- Subquery flattening (decorrelation)
- Semi-join/anti-join recognition
- Predicate pullup
- Subquery memoization

### 6. CTEs (12 queries)
Common table expressions.

**Features tested:**
- Non-recursive CTEs
- Recursive CTEs
- Multiple CTEs in single query
- CTEs referenced multiple times

**Expected optimizations:**
- CTE inlining vs materialization
- Recursive CTE optimization
- Predicate pushdown into CTEs
- Sharing across multiple references

### 7. Set Operations (11 queries)
Set operations combining multiple SELECT statements.

**Features tested:**
- UNION / UNION ALL
- INTERSECT / INTERSECT ALL
- EXCEPT / EXCEPT ALL

**Expected optimizations:**
- Pushdown predicates through set operations
- Convert UNION to UNION ALL when possible
- Merge adjacent set operations

### 8. Advanced SQL Features (9 queries)
Features with partial or experimental support.

**Features tested:**
- Window functions
- VALUES constructor
- Advanced predicates

**Expected optimizations:**
- Window sharing
- Sort order reuse
- Expression simplification

### 9. Unsupported SQL Features (8 queries)
Queries testing unsupported features (for gap analysis).

**Features tested:**
- GROUPING SETS/CUBE/ROLLUP
- LATERAL subqueries
- PIVOT/UNPIVOT
- JSON functions

**Expected behavior:**
- Parser may succeed but optimizer fails
- Graceful degradation
- Clear error messages

## Metrics Collected

### Planning Efficiency
- `plan_time_us` - Wall-clock time for Ra optimizer (microseconds)
- `pg_plan_time_us` - PostgreSQL planning time (microseconds)
- `rules_applied` - Number of rewrite rules fired
- `egraph_nodes` - E-graph nodes at saturation
- `egraph_classes` - E-graph equivalence classes
- `memory_allocated_bytes` - Memory used by e-graph

### Plan Quality
- `plan_cost_estimate` - Ra's cost estimate
- `pg_plan_cost` - PostgreSQL's cost estimate
- `q_error` - max(estimated/actual, actual/estimated)

### Feature Coverage
- `parser_success` - Query parsed successfully
- `optimizer_success` - Query optimized successfully
- `error_message` - Error details if failed

## Running the Benchmark

### Build and run
```bash
cargo build --release --bin planner_comparison_runner
cargo run --release --bin planner_comparison_runner
```

### Output
The benchmark generates two output files:

1. **metrics.json** - Raw metrics for each query in JSON format
2. **COMPARISON_REPORT.md** - Human-readable markdown report with:
   - Overall summary statistics
   - Results by category
   - Detailed query results
   - Feature coverage analysis
   - Failed queries with error messages

### Expected Runtime
- Simple queries: ~1-10ms each
- Basic joins: ~10-100ms each
- Complex joins: ~100-1000ms each
- Subqueries/CTEs: ~50-500ms each
- Total: ~5-10 minutes for all 120 queries

## Performance Targets

Based on BENCHMARK_PLAN.md targets:

| Dimension | Metric | Target | Stretch Goal |
|-----------|--------|--------|-------------|
| Planning Efficiency | Median plan time (simple) | <100ms | <50ms |
| | Median plan time (complex) | <2000ms | <1000ms |
| Planning Accuracy | Median Q-error | <2.0 | <1.5 |
| | P95 Q-error | <10.0 | <5.0 |
| Feature Coverage | Parser success | >95% | >98% |
| | Optimizer success | >90% | >95% |

## Adding New Queries

To add queries to a category:

1. Create a new `.sql` file in the appropriate category directory
2. Use descriptive filenames (e.g., `06_complex_case_when.sql`)
3. Add a comment at the top describing what the query tests
4. Re-run the benchmark to include the new query

Query naming convention:
- Use numbered prefixes (01_, 02_, etc.) for ordering
- Use descriptive names indicating what's being tested
- Keep queries focused on a single feature/optimization

## Comparison with PostgreSQL

To collect PostgreSQL planning times:

```bash
# Create a script to run EXPLAIN on all queries
cd benchmarks/planner_comparison
./collect_pg_plans.sh > pg_plans.json
```

The `collect_pg_plans.sh` script (to be implemented) will:
1. Connect to PostgreSQL
2. Run EXPLAIN on each query
3. Extract planning time and cost estimates
4. Output JSON with results

## References

- **BENCHMARK_PLAN.md** - Comprehensive benchmark framework specification
- **benchmarks/tpch-ra-vs-pg.md** - TPC-H benchmark results
- **benchmarks/job/benchmark_runner.rs** - JOB benchmark metrics framework
- **crates/ra-engine/benches/tpch_all22.rs** - TPC-H benchmark implementation
