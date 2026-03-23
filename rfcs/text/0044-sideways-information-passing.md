# RFC 0044: Sideways Information Passing (SIP)

- Start Date: 2026-03-22
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Implement Sideways Information Passing (SIP), an adaptive query processing technique that dynamically passes bloom filters and bitmaps from completed join operations to remaining scans during execution. This reduces data read from disk and rows processed by subsequent joins, providing 10x-100x speedup for multi-way joins.

## Motivation

Traditional query optimizers make a single plan decision before execution, using static statistics. When statistics are wrong or data is skewed, this leads to inefficient execution.

**Problem**: Multi-way joins with selective predicates

```sql
-- TPC-H Query 5 (simplified)
SELECT n.name, SUM(l.extendedprice)
FROM nation n
JOIN customer c ON n.nationkey = c.nationkey
JOIN orders o ON c.custkey = o.custkey
JOIN lineitem l ON o.orderkey = l.orderkey
WHERE n.name = 'FRANCE'
GROUP BY n.name
```

**Current execution**:
1. Scan nation (25 rows) -> filter to 'FRANCE' (1 row)
2. Build hash table: nation (1 row)
3. Scan customer (150K rows), probe hash, output matches (~6K rows)
4. Build hash table: matched customers (6K rows)
5. Scan orders (1.5M rows), probe hash, output matches (~90K rows)
6. Build hash table: matched orders (90K rows)
7. Scan lineitem (6M rows), probe hash, output matches (~360K rows)

**Problem**: lineitem scan reads 6M rows, but only 360K match. We knew orderkeys after step 5, why scan all lineitem?

**With SIP**:
1. After completing nation $\bowtie$ customer, create bloom filter BF_customer from matched custkeys
2. **Pass BF_customer to orders scan** -> scan orders, filter using bloom filter (~60K rows read vs 1.5M)
3. After completing orders, create bloom filter BF_orders from matched orderkeys
4. **Pass BF_orders to lineitem scan** -> scan lineitem, filter using bloom filter (~360K rows read vs 6M)

**Benefit**: 10x reduction in I/O, 10x reduction in rows processed.

### Use Cases

1. **Star Schema Joins**: Filter fact table using dimension join results
2. **TPC-H Queries**: Multi-way joins with selective predicates (Q5, Q8, Q9, Q21)
3. **Ad-hoc Analytics**: User doesn't know optimal join order, SIP adapts
4. **Skewed Data**: SIP helps even when cardinality estimates are wrong
5. **Streaming Joins**: Pass filters from completed parts of stream to future scans

## Guide-level explanation

SIP is a runtime optimization that builds filters (bloom filters or bitmaps) from join results and pushes them to remaining table scans. This happens **during execution**, not during planning.

### Example: Three-Way Join

**Query**:
```sql
SELECT *
FROM A JOIN B ON A.id = B.a_id
     JOIN C ON B.id = C.b_id
WHERE A.category = 'X'
```

**Without SIP**:
```
1. Scan A, filter to category='X' -> 100 rows
2. Join with B (scan all 1M rows) -> 500 matching rows
3. Join with C (scan all 10M rows) -> 2K matching rows
```
**Total I/O**: 1M + 10M = 11M rows

**With SIP**:
```
1. Scan A, filter to category='X' -> 100 rows
2. Build bloom filter BF_A from A.id (100 values)
3. Scan B with bloom filter BF_A -> ~500 rows (filtered at scan, not join)
4. Join A $\bowtie$ B -> 500 rows
5. Build bloom filter BF_B from B.id (500 values)
6. Scan C with bloom filter BF_B -> ~2K rows (filtered at scan)
7. Join (A $\bowtie$ B) $\bowtie$ C -> 2K rows
```
**Total I/O**: 500 + 2K = ~2.5K rows

**Speedup**: 4400x reduction in rows read!

### How It Works

1. **During join execution**, when a join build side completes, extract join keys
2. **Build a filter**: bloom filter (probabilistic) or bitmap (exact)
3. **Pass filter to remaining scans**: scans can apply filter before sending data
4. **Adapt dynamically**: If filter is not selective, don't use it

### Filter Types

**Bloom Filter** (probabilistic):
- False positive rate: 1-5%
- Size: ~10 bits per key
- Example: 1M keys = ~1.25 MB bloom filter
- **Use when**: Many distinct keys, can tolerate false positives

**Bitmap** (exact):
- No false positives
- Size: 1 bit per possible value
- Example: 1M possible values = 125 KB bitmap
- **Use when**: Low cardinality (< 1M distinct values), need exactness

### When SIP Applies

SIP helps when:
1. **Selective filter** on one relation reduces join output significantly
2. **Multi-way joins** (3+ tables) where intermediate results are small
3. **Large fact tables** that would benefit from filtering before scan
4. **Skewed data** where cardinality estimates are inaccurate

SIP does NOT help when:
1. All joins are already very selective (no benefit from additional filtering)
2. Bloom filter overhead > scan savings
3. Single-table queries (no joins)

## Reference-level explanation

### Algorithm

#### Phase 1: Execution Plan Instrumentation

During plan generation, mark potential SIP opportunities:
```rust
pub struct SIPOpportunity {
    /// Join operator that produces filter
    producer_join: JoinId,
    /// Table scan that consumes filter
    consumer_scan: ScanId,
    /// Column to filter on
    filter_column: String,
    /// Expected selectivity
    estimated_selectivity: f64,
}
```

Heuristic: Mark scans downstream of selective joins as SIP consumers.

#### Phase 2: Filter Generation (Runtime)

When a join completes its build phase:
```rust
fn generate_sip_filter(join_keys: &[Value]) -> SIPFilter {
    if join_keys.len() < BITMAP_THRESHOLD {
        // Use exact bitmap for low cardinality
        SIPFilter::Bitmap(create_bitmap(join_keys))
    } else {
        // Use bloom filter for high cardinality
        SIPFilter::BloomFilter(create_bloom_filter(join_keys, FALSE_POSITIVE_RATE))
    }
}
```

Constants:
- `BITMAP_THRESHOLD`: 100,000 (switch to bloom if more keys)
- `FALSE_POSITIVE_RATE`: 0.02 (2% false positives)

#### Phase 3: Filter Propagation

Pass filter to downstream scans:
```rust
fn propagate_filter(filter: SIPFilter, target_scan: &mut TableScan) {
    // Send filter to scan operator
    target_scan.apply_sip_filter(filter);

    // Track filter usage for adaptive decisions
    runtime_stats.record_sip_filter(filter.id(), filter.size());
}
```

#### Phase 4: Filter Application at Scan

Scan operator applies filter before emitting tuples:
```rust
impl TableScan {
    fn next_tuple(&mut self) -> Option<Tuple> {
        loop {
            let tuple = self.storage.next()?;

            // Apply SIP filters
            if !self.sip_filters.is_empty() {
                if !self.passes_sip_filters(&tuple) {
                    continue;  // Skip tuple, don't emit
                }
            }

            return Some(tuple);
        }
    }

    fn passes_sip_filters(&self, tuple: &Tuple) -> bool {
        for filter in &self.sip_filters {
            let key_value = tuple.get(&filter.column);
            if !filter.contains(key_value) {
                return false;  // Tuple filtered out
            }
        }
        true
    }
}
```

### Implementation Details

**Data Structures**:

```rust
pub enum SIPFilter {
    BloomFilter {
        bits: Vec<u64>,
        hash_count: usize,
        column: String,
    },
    Bitmap {
        bits: Vec<u8>,
        min_value: i64,
        max_value: i64,
        column: String,
    },
}

impl SIPFilter {
    fn contains(&self, value: &Value) -> bool {
        match self {
            SIPFilter::BloomFilter { bits, hash_count, .. } => {
                // Check bloom filter
                for i in 0..*hash_count {
                    let hash = hash_value(value, i) % (bits.len() * 64);
                    let word_idx = hash / 64;
                    let bit_idx = hash % 64;
                    if (bits[word_idx] & (1 << bit_idx)) == 0 {
                        return false;  // Definitely not present
                    }
                }
                true  // Probably present (may be false positive)
            }
            SIPFilter::Bitmap { bits, min_value, max_value, .. } => {
                // Check bitmap
                if let Value::Int64(v) = value {
                    if *v < *min_value || *v > *max_value {
                        return false;
                    }
                    let offset = (*v - *min_value) as usize;
                    let byte_idx = offset / 8;
                    let bit_idx = offset % 8;
                    (bits[byte_idx] & (1 << bit_idx)) != 0
                } else {
                    false
                }
            }
        }
    }

    fn estimated_benefit(&self, scan_cardinality: f64) -> f64 {
        // Benefit = rows filtered / (filter creation cost + propagation cost)
        let selectivity = self.estimated_selectivity();
        let rows_filtered = scan_cardinality * (1.0 - selectivity);
        let cost_overhead = self.size() as f64 * 0.001;  // Cost to create and send filter

        rows_filtered / cost_overhead
    }
}

pub struct RuntimeSIPStats {
    /// Filters created
    filters_created: usize,
    /// Filters applied
    filters_applied: usize,
    /// Rows filtered by SIP
    rows_filtered: u64,
    /// False positives (bloom filter only)
    false_positives: u64,
}
```

**Adaptive Decision Making**:

```rust
fn should_apply_sip(
    join_result_size: usize,
    downstream_scan_size: usize,
    filter_size_bytes: usize,
) -> bool {
    // Don't apply if join result is already very large (filter won't help)
    if join_result_size > downstream_scan_size / 10 {
        return false;
    }

    // Don't apply if filter overhead > scan cost savings
    let scan_cost_without_filter = downstream_scan_size as f64 * CPU_TUPLE_COST;
    let expected_selectivity = join_result_size as f64 / downstream_scan_size as f64;
    let scan_cost_with_filter =
        downstream_scan_size as f64 * expected_selectivity * CPU_TUPLE_COST
        + filter_size_bytes as f64 * CPU_BIT_CHECK_COST;

    scan_cost_with_filter < scan_cost_without_filter * 0.9  // At least 10% benefit
}
```

### Integration Points

**Interactions with**:
- **Parallel Execution**: Filters can be shared across workers
- **Distributed Execution**: Filters broadcast to remote nodes (small size)
- **Index Scans**: SIP can combine with bitmap index scans
- **Adaptive Query Processing**: SIP is a form of runtime adaptation

**Architecture**:
- **Volcano/Iterator Model**: Filters passed via scan operator state
- **Vectorized Execution**: Batch filter checks (SIMD-friendly)
- **Push-based Execution**: Filters sent as control messages

### Error Handling

**SIP never causes incorrect results**:
- Bloom filter false positives handled by join operator (will reject non-matches)
- Worst case: Filter doesn't help, no performance degradation

**Failures**:
- Out of memory: Skip filter creation, continue without SIP
- Filter too large: Use cheaper filter (bloom -> bitmap -> none)

### Performance Considerations

**Expected Speedup**:
- **Highly selective queries** (1% selectivity): 10x-100x faster
- **Moderately selective** (10% selectivity): 2x-5x faster
- **Low selectivity** (50%+ selectivity): Minimal benefit or overhead

**Overhead**:
- Bloom filter creation: ~0.5 $\mu$s per key
- Bloom filter check: ~0.05 $\mu$s per tuple
- Bitmap creation: ~0.1 $\mu$s per key
- Bitmap check: ~0.01 $\mu$s per tuple

**Memory**:
- Bloom filter: ~10-15 bits per key
- Bitmap: 1 bit per possible value
- Example: 1M keys = 1.25 MB bloom filter (negligible)

## Drawbacks

### Complexity Cost
- Adds ~1500 LOC for filter generation, propagation, and application
- Requires runtime state management (filters passed between operators)
- Debugging harder (behavior differs across executions based on data)

### Runtime Overhead
- Filter creation cost (hashing, bitmap generation)
- Filter checking cost (per tuple)
- Memory for storing filters (usually small: 1-10 MB)

### False Positives (Bloom Filters)
- 1-5% of tuples pass bloom filter but fail join (wasted work)
- Rare: If false positive rate is 2% and selectivity is 1%, overhead is 0.02% of scan

### Non-determinism
- Query plans behave differently based on data distribution
- Hard to reproduce performance issues (depends on runtime decisions)
- Solution: Logging and runtime statistics

## Rationale and alternatives

### Why This Design?

**SIP is proven**:
- Academic foundation: Deshpande et al. (2007), "Eddies" Avnur & Hellerstein (2000)
- Production use: Presto, Impala, Spark, Databricks
- Core technique for big data systems

**Adapts to real data**:
- Works even when cardinality estimates are wrong
- Handles skewed data distribution
- Complements static optimization

### Alternative Approaches

#### 1. Static Bloom Filters (Compile-Time)

**Approach**: Create bloom filters during planning, not execution

**Pros**: Simpler implementation (no runtime state)
**Cons**:
- Requires accurate statistics (defeats purpose of adaptation)
- Can't adapt to parameter changes (WHERE x = $1)

#### 2. Runtime Reoptimization

**Approach**: Re-plan query mid-execution when cardinality is off

**Pros**: Can change join order, not just add filters
**Cons**:
- Very expensive (replanning cost)
- Discards work done so far
- Difficult to implement

#### 3. Index-Based Filtering

**Approach**: Use indexes to avoid full scans

**Pros**: No runtime filter creation
**Cons**:
- Requires indexes to exist
- Doesn't help for multi-way joins without covering indexes

### Impact of Not Doing This

**Without SIP**:
- Multi-way joins 10x-100x slower on selective queries
- Performance depends heavily on accurate statistics (often unavailable)
- Ad-hoc queries suffer (no tuned indexes)
- Competitive disadvantage vs Presto, Spark, Impala

## Prior art

### Academic Research

**"Eddies: Continuously Adaptive Query Processing" (Avnur & Hellerstein, 2000)**
- Introduced adaptive query processing with runtime reordering
- Bloom filters used for "tuple routing"
- Foundation for SIP

**"Adaptive Optimization of Very Large Join Queries" (Deshpande et al., 2007)**
- Coined term "Sideways Information Passing"
- Showed 10x-100x speedup on TPC-H queries
- Proved correctness and cost model

**"LEO - DB2's LEarning Optimizer" (Stillger et al., 2001)**
- Runtime feedback for future queries
- Bloom filters for join optimization
- Combined with query reoptimization

### Industry Solutions

**Presto / Trino**:
- **Dynamic Filtering**: Creates bloom filters from small dimension tables
- **Broadcast**: Filters sent to all workers
- **Huge impact**: 10x+ speedup on star schema queries

**Apache Impala**:
- **Runtime Filters**: Bloom filters and min/max filters
- **Broadcast vs local**: Adaptive based on filter size
- **Critical feature**: Enabled by default

**Apache Spark**:
- **Dynamic Partition Pruning**: Uses bloom filters to skip partitions
- **Adaptive Query Execution**: Can enable/disable SIP based on cost
- **Introduced**: Spark 3.0 (2020)

**Databricks Photon**:
- **Vectorized SIP**: SIMD-optimized bloom filter checks
- **Multi-level filters**: Bloom + min/max + count
- **Performance**: Claims 5x-10x over Spark on selective joins

**PostgreSQL**:
- **No SIP support**: All optimization is static
- **Feature request**: Multiple users requested, no implementation yet

### What We Can Learn

**Key insights**:
1. **Start with broadcast**: Send filters to all workers (simplest)
2. **Cost-based decision**: Only apply if benefit > cost (use runtime cardinality)
3. **Multiple filter types**: Bloom for high cardinality, bitmap for low
4. **SIMD-friendly**: Bloom filter checks should vectorize well
5. **Logging essential**: Track filter effectiveness for debugging

**Impala lesson**: Make SIP a first-class feature, not afterthought. Default enabled.

## Unresolved questions

**Design Questions**:
1. **Filter size limit**: Max bloom filter size before rejecting? 10 MB? 100 MB?
2. **False positive rate**: Target 1%? 2%? 5%?
3. **Min benefit threshold**: Only apply if expected speedup > 2x? 5x?

**Implementation Strategy**:
1. Volcano model: Pass filters via operator state or separate channel?
2. Vectorized execution: Batch bloom filter checks (8 tuples at once)?
3. Parallel execution: Share filters across workers or per-worker?

**Integration Questions**:
1. Interaction with indexes: Apply SIP filter before or after index filter?
2. Distributed execution: Broadcast all filters or only selective ones?
3. EXPLAIN output: How to show SIP was applied? (Runtime info, not plan-time)

**Out of Scope** (for initial RFC):
- Runtime join reordering (SIP only adds filters, doesn't change order)
- Learning from past queries (LEO-style optimization)
- Complex filter types (histograms, frequency sketches)

## Future possibilities

### Natural Extensions

#### 1. Multi-Column Filters
- Pass filters on multiple join columns
- Example: (product_id, store_id) composite key
- Requires multi-dimensional bloom filters

#### 2. Min/Max Filters
- For range predicates, pass min/max bounds instead of bloom
- Example: After filtering to 2020-01-01 to 2020-12-31, pass bounds to downstream
- Cheaper than bloom for numeric ranges

#### 3. Count Sketches
- Approximate cardinality of join result
- Use to dynamically choose join algorithm (hash vs merge vs nested loop)

#### 4. Adaptive False Positive Rate
- Start with low FP rate (1%), increase if filter is too large
- Balance between filter size and selectivity

### Long-term Vision

**Fully Adaptive Query Execution**:
- SIP + runtime join reordering
- Example: If intermediate result is unexpectedly large, switch join order
- Requires executor support for plan changes mid-execution

**Learning-Based SIP**:
- Learn which queries benefit from SIP
- Store metadata: (query pattern, filter effectiveness)
- Auto-enable SIP for similar future queries

**Cross-Query Optimization**:
- Reuse SIP filters across concurrent queries
- Example: 10 users query same date range -> share filter
- Requires shared filter cache

---

## Implementation Roadmap

### Phase 1: Single-Node SIP (Bloom Filters)
- Bloom filter generation from join results
- Filter propagation to downstream scans
- Adaptive decision (benefit estimation)
- ~1000 LOC
- **Benefit**: 10x-100x speedup for selective multi-way joins

### Phase 2: Bitmap Filters + Parallel Execution
- Bitmap filters for low-cardinality columns
- Filter sharing across parallel workers
- ~500 LOC
- **Benefit**: Handles more query patterns, better parallel scaling

### Phase 3: Distributed Execution
- Broadcast filters to remote nodes
- Network cost estimation
- ~400 LOC
- **Benefit**: Enables SIP for distributed star schema queries

**Total effort**: 4-6 weeks for full implementation

---

## References

- Deshpande, A., et al. (2007). *Adaptive Optimization of Very Large Join Queries*. SIGMOD '07.
- Avnur, R., & Hellerstein, J. M. (2000). *Eddies: Continuously Adaptive Query Processing*. SIGMOD '00.
- Presto: [Dynamic Filtering](https://prestodb.io/docs/current/admin/properties.html#dynamic-filtering)
- Impala: [Runtime Filtering](https://impala.apache.org/docs/build/html/topics/impala_runtime_filtering.html)
- Spark: [Adaptive Query Execution](https://spark.apache.org/docs/latest/sql-performance-tuning.html#adaptive-query-execution)
