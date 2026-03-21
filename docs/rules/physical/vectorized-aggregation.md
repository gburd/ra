# Rule: Vectorized Aggregation

**Category:** physical/aggregation-strategies
**File:** `rules/physical/aggregation-strategies/vectorized-aggregation.rra`

## Metadata

- **ID:** `vectorized-aggregation`
- **Version:** "1.0.0"
- **Databases:** duckdb, clickhouse, umbra
- **Tags:** aggregation, vectorized, simd
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(aggregate ?input ?groups ?aggs)"
    description: "Aggregation using vectorized execution"
  - type: "capability"
    database: "current"
    requires: "vectorized_execution"
    description: "Database supports vectorized execution"
  - type: "fact"
    fact_type: "statistics.cardinality"
    table: "?input"
    comparator: ">"
    threshold: 10000
    optional: true
    description: "Input large enough to benefit from SIMD vectorization"
```


# Vectorized Aggregation

## Description

Processes aggregations on batches of rows using SIMD instructions; amortizes dispatch overhead.

**When to apply**: Columnar data with vectorizable aggregation functions.

**Why it works**: SIMD operations process multiple values simultaneously; reduces branches and function calls.

## Relational Algebra

```algebra
aggregate[group_keys, agg_funcs](columnar(R))
  -> vectorized_aggregate:
       for each vector batch B in R: // e.g., 1024 rows
         extract_group_keys_vectorized(B) -> keys
         for each aggregate func:
           vectorized_update(hash_table, keys, B.columns)
```

## Implementation

```rust
rw!("use-vectorized-aggregation";
    "(aggregate ?groups ?aggs ?input)" =>
    "(vectorized-aggregate ?groups ?aggs ?input)"
    if is_columnar("?input") && supports_simd() && vectorizable_aggs("?aggs")
),
```

## Cost Model

```rust
fn cost(input_size: u64, vector_width: usize) -> f64 {
    let batches = (input_size as f64 / vector_width as f64).ceil();
    let cost_per_batch = 10.0; // Amortized vs 100.0 per tuple
    batches * cost_per_batch
}
```

**Typical benefit**: 40-80% vs tuple-at-a-time aggregation

## Test Cases

### Positive: Numeric aggregations

```sql
SELECT category, SUM(price), AVG(quantity), COUNT(*)
FROM products
GROUP BY category;

-- Numeric columns: vectorized SUM/AVG with SIMD
```

### Positive: Simple hash keys

```sql
SELECT date, COUNT(*), SUM(amount)
FROM transactions
GROUP BY date;

-- Integer date keys: vectorized hash computation
```

### Negative: Complex expressions

```sql
SELECT SUBSTRING(name, 1, 3), COUNT(*)
FROM users
GROUP BY SUBSTRING(name, 1, 3);

-- String manipulation: hard to vectorize
```

## References

- DuckDB: Vectorized execution engine
- ClickHouse: SIMD aggregation
- "Everything You Always Wanted to Know About Compiled and Vectorized Queries But Were Afraid to Ask" (Kersten et al., 2018)
