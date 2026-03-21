# Rule: Probabilistic Sketches for Distributed Aggregation

**Category:** experimental/approximate
**File:** `rules/experimental/approximate/sketches-for-aggregation.rra`

## Metadata

- **ID:** `sketches-for-aggregation`
- **Version:** "1.0.0"
- **Databases:** duckdb, clickhouse, cockroachdb, tidb
- **Tags:** approximate, sketch, hyperloglog, count-min, aggregation, streaming
- **Authors:** "Flajolet & Martin 1985", "Cormode & Muthukrishnan 2005", "RA Contributors"


# Probabilistic Sketches for Distributed Aggregation

## Description

Replaces exact aggregation operators with probabilistic data structures
(sketches) that compute approximate results in bounded memory using a
single pass. Sketches are mergeable: partial sketches from different
nodes can be combined into a final sketch, enabling distributed
aggregation without exact deduplication.

**When to apply**: Queries that compute COUNT DISTINCT, frequency
estimates, quantiles, or set membership where approximate results
(with bounded error guarantees) are acceptable. Especially valuable
for high-cardinality GROUP BY with COUNT DISTINCT on distributed data.

**Why it works**: Exact COUNT DISTINCT requires maintaining the full
set of distinct values (O(n) memory). HyperLogLog uses O(log log n)
memory with ~0.8% relative error. In distributed execution, partial
HLL sketches are merged without transferring raw distinct values,
reducing network traffic from O(distinct values) to O(sketch size).

## Relational Algebra

```algebra
-- Exact:
gamma[g, COUNT(DISTINCT x)](R)

-- Approximate with HyperLogLog:
gamma[g, HLL_MERGE(partial_hll)](
    Exchange[hash(g)](
        gamma[g, HLL_ADD(x) AS partial_hll](R_partition)
    )
)

-- Sketch size: 2^p bytes (p=14 -> 16KB, 0.8% error)
```

## Implementation

```rust
enum SketchType {
    HyperLogLog {
        precision: u8,  // log2(num_registers), typically 14
        registers: Vec<u8>,
    },
    CountMinSketch {
        width: usize,   // columns per row
        depth: usize,   // number of hash functions
        table: Vec<Vec<i64>>,
    },
    TDigest {
        compression: f64,
        centroids: Vec<Centroid>,
    },
}

fn rewrite_count_distinct_to_hll(
    agg: &AggregationExpr,
) -> Option<AggregationExpr> {
    match agg {
        CountDistinct(col) => Some(HllCount {
            column: col.clone(),
            precision: 14,  // ~0.8% error
        }),
        _ => None,
    }
}

fn rewrite_percentile_to_tdigest(
    agg: &AggregationExpr,
) -> Option<AggregationExpr> {
    match agg {
        Percentile(col, p) => Some(TDigestPercentile {
            column: col.clone(),
            percentile: *p,
            compression: 100.0,
        }),
        _ => None,
    }
}

// Sketches are mergeable for distributed execution
trait MergeableSketch {
    fn merge(&mut self, other: &Self);
    fn estimate(&self) -> f64;
    fn memory_bytes(&self) -> usize;
    fn relative_error(&self) -> f64;
}
```

## Preconditions

```rust
fn applicable(
    agg: &AggregationExpr,
    session: &SessionConfig,
) -> bool {
    // Approximate results must be acceptable
    session.allow_approximate()
    // Aggregation must be one of the supported types
    && matches!(agg,
        CountDistinct(_) | Percentile(_, _) | Frequency(_))
    // Error bound must be within user's tolerance
    && sketch_error_bound(agg) <= session.max_error_tolerance()
}
```

**Supported sketch mappings:**
| Exact Operation | Sketch Type | Error | Memory |
|----------------|-------------|-------|--------|
| COUNT(DISTINCT x) | HyperLogLog | ~0.8% | 16KB |
| PERCENTILE(x, p) | t-digest | ~1-5% | ~10KB |
| FREQUENCY(x) | Count-Min Sketch | additive eps | w*d*8B |
| TOP-K(x) | Space-Saving | exact top-k | k entries |
| SET MEMBERSHIP(x) | Bloom Filter | false pos eps | m bits |

**Restrictions:**
- Results are approximate with probabilistic error bounds
- HyperLogLog has a small bias for low cardinalities (bias
  correction applied below 2.5 * m)
- Count-Min Sketch only provides upper-bound frequency estimates
  (can overcount, never undercount)
- t-digest accuracy varies at extreme percentiles (p < 0.01 or
  p > 0.99)
- Sketch-based aggregations are not deterministic (hash function
  randomness) -- results may vary slightly between executions

## Cost Model

```rust
fn sketch_benefit(
    distinct_values: f64,
    total_rows: f64,
    num_nodes: u32,
    exact_network_cost: f64,
    sketch_network_cost: f64,
    exact_memory: f64,
    sketch_memory: f64,
) -> f64 {
    let network_savings =
        (exact_network_cost - sketch_network_cost)
        * num_nodes as f64;
    let memory_savings = exact_memory - sketch_memory;
    network_savings + memory_savings
}
```

**Typical benefit**: COUNT(DISTINCT user_id) on 1B rows with 100M
distinct users: exact requires 100M * 8 bytes = 800MB per node;
HyperLogLog uses 16KB per node with 0.8% error.

## Test Cases

```sql
-- Positive: approximate COUNT DISTINCT
SELECT page_url, APPROX_COUNT_DISTINCT(user_id)
FROM page_views
GROUP BY page_url;

-- Exact: requires 800MB hash table for user_id dedup
-- HLL: 16KB per group, <1% error
-- In distributed: merge 16KB HLL sketches vs. 800MB hash tables
```

```sql
-- Positive: approximate percentile
SELECT region, APPROX_PERCENTILE(response_time, 0.99)
FROM requests
GROUP BY region;

-- t-digest: ~10KB per group, ~1% error at p99
-- Exact: requires sorting or large sample buffer
```

```sql
-- Positive: approximate top-k
SELECT APPROX_TOP_K(search_term, 100) FROM queries;

-- Space-Saving algorithm: exact top-100 with O(100) memory
-- vs. exact: requires full frequency count of all terms
```

```sql
-- Negative: user requires exact count
SELECT COUNT(DISTINCT customer_id) FROM orders;
-- If business requires exact number (e.g., for billing),
-- approximate is not acceptable
```

## References

Flajolet, Fusy, Gandouet, Meurisse, "HyperLogLog: the analysis of a near-optimal cardinality estimation algorithm" (AofA 2007)
Cormode & Muthukrishnan, "An Improved Data Stream Summary: The Count-Min Sketch" (JAAL 2005)
Dunning & Ertl, "Computing Extremely Accurate Quantiles Using t-Digests" (2019)
ClickHouse: uniqHLL12, quantileTDigest functions
DuckDB: APPROX_COUNT_DISTINCT, APPROX_QUANTILE
CockroachDB: experimental approximate functions
