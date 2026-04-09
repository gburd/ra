# Comparison Features

RA Web's multi-panel comparison lets you analyze query plans across different database engines, versions, and configurations.

## Why Compare Query Plans?

**Version Upgrades**

Test queries on new database versions before migrating production systems. Identify plan regressions or improvements.

**Engine Selection**

Compare PostgreSQL, MySQL, MariaDB, DuckDB, and SQLite to choose the best engine for your workload.

**Optimization Validation**

Test whether adding an index, rewriting a query, or changing configuration actually improves performance.

**Dialect Learning**

Understand how different SQL dialects interpret the same query.

## Setting Up Comparisons

### Basic Two-Panel Comparison

1. Click **Add Panel** in the toolbar (top-right)

2. Select first engine in left panel (e.g., PostgreSQL 15)

3. Select second engine in right panel (e.g., PostgreSQL 16)

4. Click **Execute** to run the query on both engines

5. Compare the results side-by-side

### Four-Panel Comparison

For comprehensive analysis, use up to four panels:

```
┌─────────────────┬─────────────────┐
│ PostgreSQL 15   │ PostgreSQL 16   │
├─────────────────┼─────────────────┤
│ MySQL 8.0       │ DuckDB          │
└─────────────────┴─────────────────┘
```

**Use cases:**

- Version comparison + dialect comparison
- Multiple optimization strategies
- Testing different index combinations (requires separate configs)

### Panel Management

**Add Panel** - Click the "+" button (max 4 panels)

**Remove Panel** - Each panel has a close button (✕) in the top-right

**Rearrange** - Panels stack automatically (2 panels: side-by-side; 3-4 panels: grid layout)

## Comparison Techniques

### Structural Comparison

Compare plan shapes and operation types.

**Tree View Comparison**

Open Tree View in all panels and observe:

1. **Join Order** - Do engines choose the same join order?
   - Left-deep vs. bushy trees
   - Driving table selection
   - Join method (hash vs. nested loop vs. merge)

2. **Operation Selection** - Which operations do engines prefer?
   - Sequential scan vs. index scan
   - Hash join vs. nested loop
   - Sort vs. hash aggregate

3. **Plan Depth** - How many levels of nesting?
   - Deeper plans may indicate more complex optimization
   - Shallower plans may be simpler but not always faster

**Example: PostgreSQL 15 vs. 16**

PostgreSQL 15:
```
Nested Loop
├── Seq Scan (employees)
└── Index Scan (departments)
```

PostgreSQL 16:
```
Hash Join
├── Seq Scan (employees)
└── Hash
    └── Seq Scan (departments)
```

**Interpretation:** PostgreSQL 16 chose a hash join (better for larger datasets), while 15 used a nested loop (better for small datasets or when one side is small).

### Cost Comparison

Compare estimated and actual costs across engines.

**Cost Analysis View**

1. Switch all panels to Cost Analysis tab

2. Compare the same operation across panels:
   - Which engine has lower total cost?
   - Which engine has better cardinality estimates?
   - Which operations differ most?

3. Look for patterns:
   - One engine consistently faster → better optimizer
   - Similar estimates but different actuals → configuration differences
   - Wildly different estimates → different statistics or cost models

**Example: Aggregate Query**

| Engine         | Est Cost | Act Cost | Variance |
|----------------|----------|----------|----------|
| PostgreSQL 16  | 234.50   | 187.23   | -20%     |
| MySQL 8.0      | 456.78   | 234.56   | -49%     |
| DuckDB         | 123.45   | 98.76    | -20%     |

**Interpretation:** DuckDB has the lowest actual cost for this aggregate query (optimized for analytics). PostgreSQL and MySQL have higher costs but more accurate estimates.

### Row Count Comparison

Examine cardinality estimates and actuals.

1. Use Raw Plan view or Cost Analysis

2. Compare estimated row counts for each operation

3. Compare actual row counts (ANALYZE mode)

4. Calculate variance: `(actual - estimated) / estimated * 100%`

**Red Flags**

- One engine has accurate estimates, others are off → better statistics
- All engines have inaccurate estimates → query complexity or data skew
- Estimates diverge significantly → different optimization algorithms

**Example: Join Query**

| Engine         | Estimated Rows | Actual Rows | Variance  |
|----------------|----------------|-------------|-----------|
| PostgreSQL 16  | 1,000          | 950         | -5%       |
| MySQL 8.0      | 500            | 950         | +90%      |
| DuckDB         | 1,200          | 950         | +26%      |

**Interpretation:** PostgreSQL has the most accurate estimate. MySQL significantly underestimated, which might lead to a suboptimal plan.

### Operation-Level Comparison

Compare specific operations across engines.

**Sequential Scan vs. Index Scan**

Some engines prefer sequential scans on small tables, others always use indexes. Compare:

- Table size threshold for index usage
- Index scan cost estimation
- Startup cost vs. total cost tradeoff

**Join Method Selection**

| Engine         | Small Join    | Medium Join  | Large Join   |
|----------------|---------------|--------------|--------------|
| PostgreSQL     | Nested Loop   | Hash Join    | Hash Join    |
| MySQL          | Nested Loop   | Block Hash   | Block Hash   |
| DuckDB         | Hash Join     | Hash Join    | Hash Join    |

**Interpretation:** DuckDB always prefers hash joins (optimized for OLAP). PostgreSQL and MySQL use nested loops for small datasets (better for OLTP).

## Statistical Comparison

For rigorous analysis, use ANALYZE mode and compare statistics.

### Execution Time

Compare actual execution time (from ANALYZE output):

```
PostgreSQL 16: Planning Time: 0.234 ms, Execution Time: 15.678 ms
MySQL 8.0:     Query Time: 23.456 ms
DuckDB:        Execution Time: 8.234 ms
```

**Metrics to compare:**

- Planning time (optimization overhead)
- Execution time (actual query processing)
- Total time (planning + execution)

### I/O Statistics

Some engines report buffer reads, cache hits, and disk I/O:

```
PostgreSQL: Shared Hit Blocks: 1234, Read Blocks: 56
MySQL:      Handler read rnd: 1234, Handler read key: 567
```

Compare I/O efficiency:
- Lower reads = better cache usage
- Higher hit ratio = better memory utilization

### Memory Usage

Compare sort/hash memory usage:

```
PostgreSQL: Sort Method: external merge  Disk: 128MB
MySQL:      Using temporary table
DuckDB:     Memory Usage: 64MB
```

**Red flags:**

- External sorts (disk usage) → increase `work_mem`
- Temporary tables → consider indexes or rewrites
- Excessive memory → may not scale to concurrent queries

## Diff View (Future Feature)

A planned enhancement will add a unified diff view:

**Side-by-Side Diff**

```
PostgreSQL 15          │ PostgreSQL 16
─────────────────────  │  ─────────────────────
Nested Loop            │ Hash Join              ← Different
├── Seq Scan           │ ├── Seq Scan           ← Same
└── Index Scan         │ └── Hash               ← Different
                       │     └── Seq Scan       ← Different
```

**Highlighting:**

- Green: Operations only in new plan
- Red: Operations removed from old plan
- Gray: Identical operations

## Best Practices

### Before Comparing

1. **Use identical queries** - Syntax differences affect plans
2. **Ensure similar data** - Row counts should match across databases
3. **Collect statistics** - Run ANALYZE on all databases
4. **Warm up caches** - Run queries twice, compare second execution
5. **Use ANALYZE mode** - Estimated costs don't reflect reality

### During Comparison

1. **Start broad, then narrow** - Tree View → Cost Analysis → Raw Plan
2. **Note configuration differences** - `work_mem`, `random_page_cost`, etc.
3. **Consider version differences** - Newer versions may have better optimizers
4. **Check for dialect issues** - Some SQL may not be portable

### Interpreting Results

**Faster doesn't always mean better:**

- Query might return wrong results (check row counts)
- May not scale (check memory usage)
- Could be luck (run multiple times for averages)

**Slower can be acceptable:**

- More accurate results (better precision)
- More reliable (less memory pressure)
- Better concurrency (fewer locks)

**Plan differences don't always matter:**

- Both plans may be equally good
- Cost models may differ but actual performance is similar
- Optimizer randomness (run EXPLAIN multiple times)

## Common Comparison Scenarios

### Scenario 1: Testing an Index

**Setup:** Compare same query with and without an index

**Method:**

1. Run query on PostgreSQL (without index)
2. Create index: `CREATE INDEX idx_name ON table(column);`
3. Run query again
4. Compare plans

**Look for:**

- Sequential scan → Index scan
- Cost reduction
- Row count accuracy (index stats may be better)

### Scenario 2: Version Upgrade Testing

**Setup:** Compare PostgreSQL 15 vs. 16 for all production queries

**Method:**

1. Load sample queries from Schema viewer
2. Execute on both versions
3. Document plan differences
4. Run ANALYZE to compare actual performance
5. Identify regressions (new version slower)

**Look for:**

- New join methods (Memoize, Incremental Sort in PG 14+)
- Cost estimate improvements
- Execution time improvements
- Memory usage changes

### Scenario 3: Engine Migration

**Setup:** Migrating from MySQL to PostgreSQL

**Method:**

1. Convert schema to PostgreSQL DDL
2. Load equivalent data
3. Compare queries side-by-side
4. Identify incompatibilities
5. Adjust queries for PostgreSQL idioms

**Look for:**

- Syntax differences (LIMIT vs. TOP)
- Plan quality differences
- Performance regressions/improvements
- Features available in one but not the other

### Scenario 4: Query Rewrite Validation

**Setup:** Test whether a query rewrite improves performance

**Method:**

1. Run original query in Panel 1
2. Run rewritten query in Panel 2 (same engine)
3. Compare plans and costs
4. Run ANALYZE to confirm improvements

**Look for:**

- Simpler plan structure
- Lower total cost
- Better cardinality estimates
- Faster execution time

## Tips

1. **Use consistent test data** - Results vary with data distribution
2. **Test at scale** - Small datasets hide problems
3. **Compare multiple queries** - One query doesn't represent workload
4. **Document findings** - Use Share button to save comparisons
5. **Iterate** - Comparison is a learning process

## Troubleshooting

**Plans look identical but performance differs**

- Check configuration (memory settings, cache size)
- Verify data is truly identical (run COUNT(*) queries)
- Test on cold caches (restart databases)
- Run multiple iterations (average times)

**Engines return different row counts**

- Check for dialect differences in WHERE clauses
- Verify NULL handling (some engines differ)
- Check for truncation differences (decimals, strings)
- Review date/time functions (formats vary)

**One engine fails to execute**

- Check SQL compatibility (use engine-specific syntax)
- Verify schema exists in that database
- Check for unsupported features (CTEs, window functions)
- Review error message in output panel

## Advanced: Scripted Comparisons

For batch testing, use the API directly:

```bash
#!/bin/bash
# Compare query across all engines

QUERY="SELECT * FROM employees WHERE department_id = 1"

for engine in postgresql-15 postgresql-16 mysql-8.0 duckdb; do
  curl -X POST http://localhost:8000/api/explain \
    -H "Content-Type: application/json" \
    -d "{\"sql\": \"$QUERY\", \"engine\": \"$engine\", \"analyze\": true}" \
    > "plan_${engine}.json"
done

# Parse and compare results
jq '.execution_time' plan_*.json
```

See [API Reference](../reference/api-reference.md) for details.
