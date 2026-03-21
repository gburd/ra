# Rule: "ClickHouse Runtime Join Bloom Filter"

**Category:** database-specific/clickhouse
**File:** `rules/database-specific/clickhouse/runtime-join-filter.rra`

## Metadata

- **ID:** `clickhouse-runtime-join-filter`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** join, bloom-filter, runtime, pushdown, semi-join
- **Authors:** "RA Contributors"


# ClickHouse Runtime Join Bloom Filter

## Metadata
- **Rule ID**: `clickhouse-runtime-join-filter`
- **Category**: Database-specific / ClickHouse
- **Source**: `src/Processors/QueryPlan/Optimizations/joinRuntimeFilter.cpp`
- **Complexity**: O(m) build + O(n) probe
- **Prerequisites**: Hash join; build side small enough for Bloom filter
- **Alternatives**: Standard hash join without pre-filtering

## Description

During hash join execution, ClickHouse builds a Bloom filter from the
join keys of the right (build) side. This Bloom filter is then pushed
down to the left (probe) side as an additional filter condition. The
filter can be propagated all the way to the storage layer, enabling
granule-level or partition-level pruning of rows that will not match
the join.

This is a dynamic optimization: the Bloom filter is constructed at
runtime from actual data, making it effective even when the optimizer
has no advance statistics about the join key distribution.

**When to apply:**
- Hash join with selective build side
- Build side significantly smaller than probe side
- Probe side is a large fact table with storage-level filtering

**Why it works for OLAP:**
- Star schema: small dimension tables join to large fact tables
- Bloom filter pushdown eliminates fact table rows at storage level
- Avoids reading and hashing non-matching probe rows

## Relational Algebra

```
A HASH JOIN B ON A.id = B.a_id
  -> bloom_filter(A, build_bloom(B.a_id)) HASH JOIN B ON A.id = B.a_id
```

## Implementation (egg rewrite rules)

```lisp
;; Build Bloom filter from join build side and push to probe side
(rewrite (hash-join ?cond ?probe ?build)
  (hash-join ?cond
    (bloom-filter (join-key-left ?cond)
      (build-bloom-filter (join-key-right ?cond) ?build)
      ?probe)
    ?build)
  :if (< (row-count ?build) (* 0.1 (row-count ?probe)))
  :if (is-hash-join-type ?cond))

;; Push Bloom filter to storage
(rewrite (bloom-filter ?col ?bf (scan ?table))
  (scan-with-bloom ?table ?col ?bf)
  :if (is-mergetree-table ?table))
```

## Cost Model

```rust
pub fn cost_runtime_bloom_filter(
    build_rows: u64,
    probe_rows: u64,
    false_positive_rate: f64,
    hardware: &HardwareModel,
) -> Cost {
    let build_cost = Cost::cpu(build_rows * 15);
    let probe_cost = Cost::cpu(probe_rows * 3);
    let surviving = (probe_rows as f64 * false_positive_rate) as u64;
    let io_savings = Cost::io(
        (probe_rows - surviving) as f64 * hardware.seq_read_cost()
    );
    build_cost + probe_cost - io_savings
}
```

**Typical benefit**: 20-80% for star schema joins with selective dimensions

## Test Cases

### Positive: Star schema with selective dimension filter
```sql
SELECT sum(f.amount) FROM fact_sales f
JOIN dim_product p ON f.product_id = p.id
WHERE p.category = 'Electronics';

-- Build Bloom filter from dim_product IDs where category = 'Electronics'
-- Push to fact_sales scan: skip granules with no matching product_ids
```

### Negative: Build side larger than probe side
```sql
SELECT * FROM small_table s
JOIN large_table l ON s.id = l.s_id;

-- large_table is build side: Bloom filter too large
-- No benefit from pushing filter to small_table
```

## References

- ClickHouse: `src/Processors/QueryPlan/Optimizations/joinRuntimeFilter.cpp`
- ClickHouse: `src/Processors/QueryPlan/BuildRuntimeFilterStep.h`
