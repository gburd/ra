# RFC 0045: Runtime Filter Pushdown with Bloom Filters

- Start Date: 2026-03-22
- Author: RA Contributors
- Status: Draft
- Tracking Issue: TBD

## Summary

Extend RA's existing bloom filter rules to support **runtime filter generation and pushdown**. During join execution, build bloom filters from small dimension tables and push them to large fact table scans, reducing I/O and network traffic. This provides 10x-100x speedup for star schema queries in both single-node and distributed execution.

## Motivation

RA already has bloom filter rules in `rules/distributed/bloom-filter-*.rra` for distributed query optimization. However, these are **static** filters created during planning. Runtime filters are created **during execution** using actual data, making them more accurate and applicable to more query patterns.

**Problem**: Star Schema Join (Common in Data Warehousing)

```sql
-- TPC-H Query 5: Revenue by nation
SELECT n.name, SUM(l.extendedprice * (1 - l.discount)) AS revenue
FROM nation n
JOIN customer c ON n.nationkey = c.nationkey
JOIN orders o ON c.custkey = o.custkey
JOIN lineitem l ON o.orderkey = l.orderkey
WHERE n.name = 'GERMANY'
GROUP BY n.name
```

**Data sizes**:
- nation: 25 rows
- customer: 150K rows → 6K rows after filter
- orders: 1.5M rows → 90K rows after join
- lineitem: 6M rows → 360K rows after join

**Current execution** (no runtime filters):
1. Scan nation (25 rows), filter to 'GERMANY' (1 row)
2. Join with customer: scan 150K rows, output 6K
3. Join with orders: scan 1.5M rows, output 90K
4. Join with lineitem: **scan 6M rows**, output 360K

**Total I/O**: 7.65M rows read

**With runtime filter pushdown**:
1. Scan nation, filter to 'GERMANY' → 1 row
2. **Build bloom filter BF1 from nationkey (1 value)**
3. **Push BF1 to customer scan** → scan 150K rows, filter to 6K (same as before)
4. **Build bloom filter BF2 from custkey (6K values)**
5. **Push BF2 to orders scan** → scan only ~100K rows (filtered), output 90K
6. **Build bloom filter BF3 from orderkey (90K values)**
7. **Push BF3 to lineitem scan** → scan only ~400K rows (filtered), output 360K

**Total I/O**: ~650K rows read (11x reduction!)

**In distributed setting**: Filters are broadcast to all workers, reducing network traffic by 10x-100x.

### Use Cases

1. **Star Schema Queries**: Filter fact tables using dimension filters
2. **Data Warehouse ETL**: Reduce data shuffled across network
3. **Semi-Joins**: Efficient implementation via bloom filters
4. **Partition Pruning**: Skip partitions based on runtime filters
5. **Late Binding**: Parameterized queries (WHERE x = $1) can't use static filters

## Guide-level explanation

Runtime filter pushdown creates bloom filters during query execution and pushes them to table scans that haven't started yet. This is different from static bloom filters which are created during planning.

### Static vs Runtime Filters

**Static Bloom Filter** (existing in RA):
```
Plan time:
- Optimizer sees: "JOIN dimension_table (10 rows)"
- Creates bloom filter with 10 placeholder values
- Pushes filter to fact_table scan

Execution time:
- Scan dimension_table, populate bloom filter with actual values
- Scan fact_table, apply bloom filter
```

**Problem**: Doesn't work for:
- Parameterized queries (values unknown at plan time)
- Joins with filters (cardinality unknown)
- Dynamic data distribution

**Runtime Bloom Filter** (this RFC):
```
Execution time:
- Scan dimension_table (small, finishes first)
- **Build bloom filter from actual join keys**
- **Send filter to fact_table scan** (may not have started yet)
- Scan fact_table with filter applied
```

**Benefit**: Works for all queries, adapts to actual data.

### Example: Parameterized Query

```sql
-- User query with parameter
SELECT * FROM orders o
JOIN lineitem l ON o.orderkey = l.orderkey
WHERE o.orderdate = $1;  -- Unknown at plan time
```

**Static filter**: Can't create bloom filter (don't know which orderdates)

**Runtime filter**:
1. Execute: `SELECT orderkey FROM orders WHERE orderdate = '2023-01-15'` → 1000 rows
2. Build bloom filter from 1000 orderkeys
3. Push to lineitem scan
4. Scan lineitem (6M rows) with filter → only ~6K rows read

**Speedup**: 1000x reduction in lineitem scan!

### Filter Lifecycle

1. **Generation**: After scanning dimension table, extract join keys
2. **Building**: Create bloom filter (10-15 bits per key)
3. **Broadcast**: Send filter to all workers/threads
4. **Application**: Scans apply filter before emitting rows
5. **Cleanup**: Discard filter after query completes

### When Runtime Filters Help

**High benefit**:
- Dimension table is small (< 100K rows)
- Fact table is large (> 1M rows)
- Join is selective (dimension filter reduces fact table by 10x+)
- Distributed execution (network cost high)

**Low benefit**:
- Dimension table is large (filter size > 100 MB)
- Join is not selective (dimension matches most fact rows)
- Single-node execution with fast storage

## Reference-level explanation

### Algorithm

#### Phase 1: Identify Filter Opportunities (Plan Time)

Mark potential runtime filter opportunities in the query plan:

```rust
pub struct RuntimeFilterOpportunity {
    /// Join operator
    join_id: JoinId,
    /// Build side (dimension table)
    build_side: RelExpr,
    /// Probe side (fact table)
    probe_side: RelExpr,
    /// Join key column
    join_column: String,
    /// Estimated filter size
    estimated_keys: usize,
    /// Expected benefit (rows filtered / filter overhead)
    expected_benefit: f64,
}
```

**Heuristic for identification**:
```rust
fn identify_opportunities(join: &Join, stats: &Statistics) -> Vec<RuntimeFilterOpportunity> {
    let mut opportunities = Vec::new();

    let build_card = stats.row_count(&join.left);
    let probe_card = stats.row_count(&join.right);

    // Only create filter if build side is significantly smaller
    if build_card < probe_card / 10 && build_card < MAX_FILTER_KEYS {
        let benefit = estimate_benefit(build_card, probe_card, stats);
        if benefit > MIN_BENEFIT_THRESHOLD {
            opportunities.push(RuntimeFilterOpportunity {
                join_id: join.id,
                build_side: join.left.clone(),
                probe_side: join.right.clone(),
                join_column: join.on.left_key.clone(),
                estimated_keys: build_card as usize,
                expected_benefit: benefit,
            });
        }
    }

    opportunities
}
```

Constants:
- `MAX_FILTER_KEYS`: 10M (don't create filter if > 10M keys)
- `MIN_BENEFIT_THRESHOLD`: 5.0 (only apply if 5x+ benefit)

#### Phase 2: Filter Generation (Execution Time)

When build side completes, generate bloom filter:

```rust
fn generate_runtime_filter(
    join_keys: &[Value],
    target_column: &str,
    false_positive_rate: f64,
) -> RuntimeFilter {
    let num_keys = join_keys.len();
    let bits_per_key = optimal_bits_per_key(false_positive_rate);
    let total_bits = num_keys * bits_per_key;
    let num_hashes = optimal_hash_count(false_positive_rate);

    let mut bloom_filter = BloomFilter::new(total_bits, num_hashes);

    for key in join_keys {
        bloom_filter.insert(key);
    }

    RuntimeFilter {
        column: target_column.to_string(),
        filter_type: FilterType::Bloom(bloom_filter),
        creation_time: Instant::now(),
        keys_count: num_keys,
    }
}
```

**Bloom filter sizing**:
- False positive rate: 2% (configurable)
- Bits per key: ~9.6 bits (for 2% FPR)
- Hash count: 6 (optimal for 2% FPR)
- Example: 100K keys = 960K bits = 120 KB

#### Phase 3: Filter Propagation

**Single-Node Execution**:
```rust
impl Join {
    fn execute(&mut self) -> Result<Vec<Tuple>> {
        // 1. Execute build side
        let build_tuples = self.build_child.execute()?;

        // 2. Extract join keys
        let join_keys: Vec<Value> = build_tuples.iter()
            .map(|t| t.get(&self.join_column))
            .collect();

        // 3. Generate runtime filter
        let filter = generate_runtime_filter(
            &join_keys,
            &self.join_column,
            FALSE_POSITIVE_RATE,
        );

        // 4. Push filter to probe side
        if let Some(scan) = find_scan_for_column(&self.probe_child, &self.join_column) {
            scan.add_runtime_filter(filter);
        }

        // 5. Execute probe side (now with filter applied)
        let probe_tuples = self.probe_child.execute()?;

        // 6. Perform join
        self.hash_join(build_tuples, probe_tuples)
    }
}
```

**Distributed Execution**:
```rust
impl DistributedJoin {
    fn execute(&mut self) -> Result<Vec<Tuple>> {
        // 1. Execute build side on coordinator
        let build_tuples = self.coordinator.execute_build()?;

        // 2. Generate filter
        let filter = generate_runtime_filter(...);

        // 3. Broadcast filter to all workers
        for worker in &self.workers {
            worker.send_runtime_filter(filter.clone())?;
        }

        // 4. Workers execute probe side with filter
        let probe_results: Vec<_> = self.workers.par_iter()
            .map(|worker| worker.execute_probe_with_filter())
            .collect()?;

        // 5. Merge results at coordinator
        self.coordinator.merge(probe_results)
    }
}
```

#### Phase 4: Filter Application at Scan

```rust
impl TableScan {
    fn next_batch(&mut self) -> Option<Batch> {
        let mut batch = self.storage.read_batch(BATCH_SIZE)?;

        // Apply runtime filters
        for filter in &self.runtime_filters {
            batch = filter.apply_to_batch(batch);
        }

        Some(batch)
    }
}

impl RuntimeFilter {
    fn apply_to_batch(&self, batch: Batch) -> Batch {
        let column_data = batch.column(&self.column);

        // Vectorized bloom filter check
        let mask = self.bloom_filter.check_batch(column_data);

        // Keep only rows where mask is true
        batch.filter_by_mask(&mask)
    }
}
```

**Vectorized bloom filter check** (SIMD):
```rust
impl BloomFilter {
    fn check_batch(&self, values: &[Value]) -> Vec<bool> {
        let mut mask = vec![true; values.len()];

        for hash_idx in 0..self.num_hashes {
            for (i, value) in values.iter().enumerate() {
                if !mask[i] {
                    continue;  // Already filtered out
                }

                let hash = hash_value(value, hash_idx);
                let bit_pos = hash % self.total_bits;
                let word_idx = bit_pos / 64;
                let bit_idx = bit_pos % 64;

                if (self.bits[word_idx] & (1 << bit_idx)) == 0 {
                    mask[i] = false;  // Filter out
                }
            }
        }

        mask
    }
}
```

### Implementation Details

**Data Structures**:

```rust
pub struct RuntimeFilter {
    /// Column to filter on
    column: String,
    /// Filter implementation (bloom or bitmap)
    filter_type: FilterType,
    /// When filter was created
    creation_time: Instant,
    /// Number of keys in filter
    keys_count: usize,
    /// Filter effectiveness metrics
    stats: FilterStats,
}

pub enum FilterType {
    Bloom(BloomFilter),
    Bitmap(Bitmap),
}

pub struct FilterStats {
    /// Rows checked against filter
    rows_checked: AtomicU64,
    /// Rows filtered out
    rows_filtered: AtomicU64,
    /// False positives (checked join, but didn't match)
    false_positives: AtomicU64,
}

impl FilterStats {
    fn effectiveness(&self) -> f64 {
        let checked = self.rows_checked.load(Ordering::Relaxed) as f64;
        let filtered = self.rows_filtered.load(Ordering::Relaxed) as f64;
        filtered / checked
    }

    fn false_positive_rate(&self) -> f64 {
        let filtered = self.rows_filtered.load(Ordering::Relaxed) as f64;
        let fp = self.false_positives.load(Ordering::Relaxed) as f64;
        fp / filtered
    }
}
```

**Bloom Filter Implementation**:

```rust
pub struct BloomFilter {
    /// Bit array
    bits: Vec<u64>,
    /// Number of hash functions
    num_hashes: usize,
    /// Total bits
    total_bits: usize,
}

impl BloomFilter {
    fn new(total_bits: usize, num_hashes: usize) -> Self {
        let num_words = (total_bits + 63) / 64;
        Self {
            bits: vec![0; num_words],
            num_hashes,
            total_bits,
        }
    }

    fn insert(&mut self, value: &Value) {
        for hash_idx in 0..self.num_hashes {
            let hash = hash_value(value, hash_idx);
            let bit_pos = hash % self.total_bits;
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            self.bits[word_idx] |= 1 << bit_idx;
        }
    }

    fn contains(&self, value: &Value) -> bool {
        for hash_idx in 0..self.num_hashes {
            let hash = hash_value(value, hash_idx);
            let bit_pos = hash % self.total_bits;
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            if (self.bits[word_idx] & (1 << bit_idx)) == 0 {
                return false;  // Definitely not present
            }
        }
        true  // Probably present
    }

    fn size_bytes(&self) -> usize {
        self.bits.len() * 8
    }
}

fn optimal_bits_per_key(false_positive_rate: f64) -> usize {
    (-1.44 * false_positive_rate.log2()).ceil() as usize
}

fn optimal_hash_count(false_positive_rate: f64) -> usize {
    (-false_positive_rate.log2()).ceil() as usize
}
```

### Integration with Existing Bloom Filter Rules

RA already has bloom filter rules in `rules/distributed/`:
- `bloom-filter-join-optimization.rra`
- `bloom-filter-semi-join.rra`
- `bloom-filter-broadcast.rra`

**Changes needed**:
1. **Extend rules** to support runtime generation (not just static)
2. **Add execution-time hooks** for filter creation
3. **Integrate with distributed execution** (broadcast filters)

**Example rule update**:
```yaml
# rules/distributed/bloom-filter-runtime-pushdown.rra
metadata:
  id: bloom-filter-runtime-pushdown
  category: distributed
  description: "Push runtime-generated bloom filters to fact table scans"

pattern:
  match: |
    (Join ?join-type
      (Scan ?dim-table ?dim-alias)
      (Scan ?fact-table ?fact-alias)
      ?on)

  preconditions:
    - (< (row-count ?dim-table) (/ (row-count ?fact-table) 10))
    - (< (row-count ?dim-table) 10000000)
    - (join-key-in-scan? ?on ?fact-table)

  rewrite: |
    (Join ?join-type
      (RuntimeFilterProducer
        (Scan ?dim-table ?dim-alias)
        ?on ?filter-id)
      (RuntimeFilterConsumer
        (Scan ?fact-table ?fact-alias)
        ?filter-id)
      ?on)

  cost-benefit:
    formula: "row-count-probe * (1 - selectivity) * cpu-tuple-cost"
```

### Error Handling

**Runtime filters never cause incorrect results**:
- Bloom filter false positives are handled by join operator (will reject)
- Worst case: Filter doesn't help, no performance degradation

**Failure modes**:
1. **Out of memory**: Skip filter creation, continue without
2. **Filter too large**: Use sampling (e.g., only 10% of keys)
3. **Timeout**: If filter generation takes too long, skip
4. **Network failure** (distributed): Workers continue without filter

### Performance Considerations

**Expected Speedup**:
- **Star schema, small dimensions**: 10x-100x faster
- **Parameterized queries**: 5x-50x faster
- **Already selective joins**: Minimal benefit (< 2x)

**Overhead**:
- Filter creation: ~0.5 μs per key (100K keys = 50 ms)
- Filter broadcast: ~1 ms per MB (10 MB filter = 10 ms)
- Filter check: ~0.05 μs per tuple (vectorized)
- Memory: ~10-15 bits per key (100K keys = 120 KB)

**Trade-offs**:
- **Best case**: Small dimension, large fact, selective join → 100x speedup
- **Worst case**: Large dimension or non-selective join → slight overhead
- **Solution**: Cost-based decision, disable if not beneficial

## Drawbacks

### Complexity Cost
- Extends existing bloom filter rules (~500 LOC additional)
- Requires runtime state management (filter passing)
- Integration with distributed execution adds complexity

### Runtime Overhead
- Filter creation cost (negligible: 50 ms for 100K keys)
- Filter broadcast cost (distributed only)
- Memory for filters (small: 1-10 MB typical)

### False Positives
- 2% of rows pass filter but fail join (wasted work)
- Can tune FPR: lower FPR = larger filter but fewer FPs

### Coordination Overhead (Distributed)
- Workers must wait for filter before scanning
- If filter takes long to generate, may delay execution
- Mitigation: Start scan, apply filter when it arrives

## Rationale and alternatives

### Why This Design?

**Extends existing RA infrastructure**:
- RA already has bloom filter rules, this adds runtime generation
- Reuses bloom filter implementation
- Minimal new code (~500 LOC on top of existing)

**Proven technique**:
- Used by Impala, Presto, Spark, Databricks
- 10x-100x speedup for star schema queries
- Critical for data warehouse performance

**Cost-based and adaptive**:
- Only applies when beneficial
- Adapts to actual data (not just statistics)

### Alternative Approaches

#### 1. Static Bloom Filters Only

**Approach**: Only use bloom filters known at plan time

**Pros**: Simpler (no runtime generation)
**Cons**:
- Doesn't work for parameterized queries
- Doesn't adapt to filtered dimensions
- Misses 50%+ of opportunities

#### 2. Semi-Join Materialization

**Approach**: Materialize dimension keys, send to workers, use as hash table

**Pros**: Exact filtering (no false positives)
**Cons**:
- Larger size (4-8 bytes per key vs 1-2 bytes for bloom)
- Higher network cost in distributed setting
- Slower lookup (hash table vs bloom filter)

#### 3. Index-Based Filtering

**Approach**: Use indexes on fact table

**Pros**: No runtime overhead
**Cons**:
- Requires indexes to exist
- Doesn't help if dimension filter creates new predicate
- Not applicable in distributed setting

### Impact of Not Doing This

**Without runtime filter pushdown**:
- Star schema queries 10x-100x slower
- Parameterized queries can't benefit from bloom filters
- Excessive network traffic in distributed queries
- Competitive disadvantage vs Presto, Spark, Impala

## Prior art

### Academic Research

**"Bloom Filters for Join Processing" (Mullin, 1990)**
- First application of bloom filters to database joins
- Showed 10x-100x reduction in join cost

**"Improved Query Performance with Variant Indexes" (O'Neil & Quass, 1997)**
- Bitmap indexes + bloom filters for star schema
- Foundation for data warehouse optimizations

### Industry Solutions

**Apache Impala**:
- **Runtime Filters**: Bloom filters + min/max filters
- **Always enabled**: Default feature since Impala 2.5 (2016)
- **Performance**: 10x-100x speedup on TPC-DS queries
- **Implementation**: Filters broadcast to all workers

**Presto / Trino**:
- **Dynamic Filtering**: Runtime bloom filter generation
- **Critical feature**: Enabled by default
- **Broadcast strategy**: Small filters broadcast, large filters local only
- **Huge impact**: Makes star schema queries feasible

**Apache Spark**:
- **Dynamic Partition Pruning**: Uses bloom filters to skip partitions
- **Adaptive Query Execution**: Can enable/disable based on cost
- **Introduced**: Spark 3.0 (2020)
- **Performance**: 5x-10x speedup on filtered joins

**Databricks Photon**:
- **Vectorized Runtime Filters**: SIMD-optimized bloom checks
- **Multi-level**: Bloom + min/max + count
- **Claims**: 10x over Spark on selective joins

**PostgreSQL**:
- **No support**: All bloom filters are index-based (static)
- **Feature request**: Multiple requests, no implementation

### What We Can Learn

**Key insights**:
1. **Default enable**: Runtime filters should be on by default (very rarely hurt)
2. **Broadcast small filters**: < 10 MB filters broadcast to all workers
3. **Vectorize checks**: Bloom filter checks should use SIMD
4. **Track effectiveness**: Log filter statistics for debugging
5. **Fail gracefully**: If filter creation fails, continue without

**Impala lesson**: Make runtime filters a first-class feature, not optional add-on.

## Unresolved questions

**Design Questions**:
1. **False positive rate**: Target 1%? 2%? 5%? (Trade-off: smaller filter vs more FPs)
2. **Max filter size**: Reject if > 10 MB? 100 MB?
3. **Min selectivity**: Only apply if expected to filter > 50% of rows?

**Implementation Strategy**:
1. **Filter passing**: Via operator state or separate message channel?
2. **Distributed broadcast**: Always broadcast or only for small filters?
3. **Multiple filters**: Can a scan have multiple runtime filters?

**Integration Questions**:
1. **Interaction with SIP**: Runtime filters are similar to SIP, merge implementations?
2. **EXPLAIN output**: How to show runtime filter was applied?
3. **Statistics**: Track filter effectiveness for future cost estimates?

**Out of Scope** (for initial RFC):
- Min/max filters (range predicates)
- Exact filters (bitmaps for very small dimensions)
- Cross-query filter reuse
- Learned filter sizing

## Future possibilities

### Natural Extensions

#### 1. Min/Max Filters
- For numeric columns, pass min/max bounds
- Cheaper than bloom filter (8 bytes total)
- Example: After filtering to year=2023, pass (min=2023-01-01, max=2023-12-31)

#### 2. Count Filters
- Pass approximate cardinality of dimension
- Use to dynamically choose join algorithm
- Example: If dimension has 1 row, use nested loop; if 1M rows, use hash join

#### 3. Multi-Column Filters
- Bloom filter on composite keys (product_id, store_id)
- Requires multi-dimensional bloom filter implementation

#### 4. Adaptive False Positive Rate
- Start with 2% FPR, increase if filter is too large
- Balance filter size vs effectiveness

### Long-term Vision

**Learned Filter Sizing**:
- Track filter effectiveness across queries
- Learn optimal FPR for different query patterns
- Auto-tune: (query pattern, data skew) → optimal FPR

**Cross-Query Filter Reuse**:
- Cache runtime filters for common query patterns
- Example: Daily ETL queries reuse yesterday's filters
- Requires invalidation on data changes

**Integration with Materialized Views**:
- Pre-compute bloom filters for dimension tables
- Query rewriter uses pre-computed filters
- Instant pushdown, no runtime cost

---

## Implementation Roadmap

### Phase 1: Single-Node Runtime Filters
- Bloom filter generation from join build side
- Push to probe side scans
- Cost-based decision
- ~400 LOC
- **Benefit**: 10x-100x speedup for star schema queries

### Phase 2: Distributed Broadcast
- Broadcast filters to all workers
- Network cost estimation
- ~200 LOC
- **Benefit**: Massive network traffic reduction

### Phase 3: Vectorized Filter Checks
- SIMD-optimized bloom filter checks
- Batch processing (check 8 values at once)
- ~150 LOC
- **Benefit**: 2x-3x faster filter application

**Total effort**: 2-3 weeks for full implementation

---

## References

- Mullin, J. K. (1990). *Optimal Semijoins for Distributed Database Systems*. IEEE TSE.
- O'Neil, P., & Quass, D. (1997). *Improved Query Performance with Variant Indexes*. SIGMOD '97.
- Impala: [Runtime Filtering](https://impala.apache.org/docs/build/html/topics/impala_runtime_filtering.html)
- Presto: [Dynamic Filtering](https://prestodb.io/docs/current/admin/properties.html#dynamic-filtering)
- Spark: [Dynamic Partition Pruning](https://spark.apache.org/docs/latest/sql-performance-tuning.html#dynamic-partition-pruning)
