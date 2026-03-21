# Rule: Adaptive Runtime Bloom Filter

**Category:** execution-models
**File:** `rules/execution-models/adaptive/adaptive-bloom-filter.rra`

## Metadata

- **ID:** `adaptive-bloom-filter`
- **Version:** 1.0.0
- **Databases:** Oracle, Spark, ClickHouse, Presto
- **Tags:** execution, adaptive, bloom-filter, join, semi-join, runtime
- **SQL Standard:** Runtime join filtering


# Adaptive Runtime Bloom Filter

## Description

Adaptive runtime Bloom filters are constructed during the build phase of a hash join and then pushed down to the probe side's scan operator to filter rows before they enter the join pipeline. The filter is built from actual join key values observed at runtime (not from statistics), so it reflects the true data distribution including any upstream filter effects.

The adaptive aspect involves two decisions: (1) whether to build a filter at all (skip if build side is too large or selectivity would be poor), and (2) how to size the filter based on the observed number of distinct keys. Oracle calls these "Bloom filter pruning" and Spark calls them "dynamic partition pruning." ClickHouse implements a similar concept as "runtime join filter."

**Key characteristics:**
- **Runtime construction**: Built from actual join key values during hash table build
- **Push-down**: Deployed to probe-side scan to filter before reading/shipping data
- **Size adaptation**: Bloom filter sized based on actual distinct key count
- **False positive tuning**: Target ~1% FPR, adjusted based on memory budget
- **Multi-join propagation**: Filters chain across multiple joins

**Trade-offs:**
- Build overhead: proportional to distinct key count
- Memory: 8-10 bits per distinct key for 1% FPR
- False positives: some non-matching rows pass through (wasted work)
- Latency: build-side must finish before filter deployed (pipeline breaker)
- Skip when build side is large (low selectivity, filter won't help)

## Relational Algebra

```
AdaptiveBloomJoin(build, probe, condition) -> Result

fn execute(build, probe, cond):
  // Phase 1: Build hash table and Bloom filter simultaneously
  ht = HashTable::new()
  distinct_keys = 0

  for row in build:
    key = cond.build_key(row)
    if ht.insert(key, row):  // returns true if new key
      distinct_keys += 1

  // Phase 2: Decide whether to deploy filter
  if should_build_filter(distinct_keys, probe.estimated_rows()):
    bf = BloomFilter::new(distinct_keys, TARGET_FPR)
    for key in ht.keys():
      bf.insert(key)
    probe.push_filter(bf)  // Push to scan operator

  // Phase 3: Probe (with or without pre-filtering)
  results = []
  for row in probe:  // probe already filtered if bf deployed
    key = cond.probe_key(row)
    for match in ht.lookup(key):
      results.push(join(match, row))

  return results
```

## Implementation

```rust
/// Bloom filter with adaptive sizing
pub struct AdaptiveBloomFilter {
    bits: Vec<u64>,
    num_bits: usize,
    num_hashes: usize,
    elements_inserted: usize,
}

impl AdaptiveBloomFilter {
    /// Create filter sized for expected elements and FPR
    pub fn new(
        expected_elements: usize,
        target_fpr: f64,
    ) -> Self {
        // Optimal bits: -n * ln(p) / (ln2)^2
        let num_bits = (-(expected_elements as f64)
            * target_fpr.ln()
            / (2.0_f64.ln().powi(2)))
            .ceil() as usize;
        let num_bits = num_bits.max(64);

        // Optimal hashes: (m/n) * ln2
        let num_hashes = ((num_bits as f64
            / expected_elements as f64)
            * 2.0_f64.ln())
            .ceil() as usize;
        let num_hashes = num_hashes.clamp(1, 16);

        let words = (num_bits + 63) / 64;

        Self {
            bits: vec\![0u64; words],
            num_bits,
            num_hashes,
            elements_inserted: 0,
        }
    }

    pub fn insert(&mut self, key: u64) {
        for i in 0..self.num_hashes {
            let hash = self.compute_hash(key, i);
            let bit_idx = hash % self.num_bits;
            self.bits[bit_idx / 64] |= 1 << (bit_idx % 64);
        }
        self.elements_inserted += 1;
    }

    pub fn might_contain(&self, key: u64) -> bool {
        for i in 0..self.num_hashes {
            let hash = self.compute_hash(key, i);
            let bit_idx = hash % self.num_bits;
            if self.bits[bit_idx / 64] & (1 << (bit_idx % 64))
                == 0
            {
                return false;
            }
        }
        true
    }

    fn compute_hash(&self, key: u64, seed: usize) -> usize {
        // Double hashing: h(i) = h1 + i * h2
        let h1 = key.wrapping_mul(0x9E3779B97F4A7C15);
        let h2 = key.wrapping_mul(0x517CC1B727220A95);
        (h1.wrapping_add(
            (seed as u64).wrapping_mul(h2),
        )) as usize
    }

    pub fn memory_bytes(&self) -> usize {
        self.bits.len() * 8
    }

    pub fn estimated_fpr(&self) -> f64 {
        let fill_ratio = 1.0
            - (-(self.elements_inserted as f64
                * self.num_hashes as f64)
                / self.num_bits as f64)
                .exp();
        fill_ratio.powi(self.num_hashes as i32)
    }
}

/// Decision: should we build a Bloom filter for this join?
pub fn should_build_bloom_filter(
    build_distinct_keys: usize,
    probe_estimated_rows: usize,
    probe_selectivity_without_filter: f64,
    memory_budget_bytes: usize,
) -> bool {
    // Skip if build side is too large (filter won't be selective)
    let selectivity = build_distinct_keys as f64
        / probe_estimated_rows as f64;
    if selectivity > 0.5 {
        return false; // Filter passes >50% of rows
    }

    // Skip if memory cost exceeds budget
    let filter_bytes = (build_distinct_keys as f64 * 10.0)
        .ceil() as usize; // ~10 bits per key for 1% FPR
    if filter_bytes > memory_budget_bytes {
        return false;
    }

    // Skip if probe side is already very selective
    if probe_selectivity_without_filter < 0.01 {
        return false; // Probe already filters 99%
    }

    // Benefit: rows eliminated * per-row-join-cost
    let rows_eliminated = probe_estimated_rows as f64
        * (1.0 - selectivity);
    let per_row_savings = 0.001; // ms per row (hash probe)
    let build_cost = build_distinct_keys as f64 * 0.0001;

    rows_eliminated * per_row_savings > build_cost
}

/// Adaptive join with runtime Bloom filter
pub struct BloomFilterJoin {
    build: Box<dyn Iterator<Item = Row>>,
    probe: Box<dyn Iterator<Item = Row>>,
    condition: JoinCondition,
    filter_budget_bytes: usize,
}

impl BloomFilterJoin {
    pub fn execute(&mut self) -> Result<Vec<Row>> {
        // Build hash table, track distinct keys
        let mut ht = HashTable::new();
        let mut distinct_count: usize = 0;

        while let Some(row) = self.build.next() {
            let key = self.condition.build_key(&row);
            if ht.insert_new(key, row) {
                distinct_count += 1;
            }
        }

        // Build Bloom filter if beneficial
        let bloom = if should_build_bloom_filter(
            distinct_count,
            self.probe_estimate(),
            1.0,
            self.filter_budget_bytes,
        ) {
            let mut bf = AdaptiveBloomFilter::new(
                distinct_count,
                0.01,
            );
            for key in ht.keys() {
                bf.insert(*key);
            }
            Some(bf)
        } else {
            None
        };

        // Probe with optional Bloom pre-filter
        let mut results = Vec::new();
        while let Some(row) = self.probe.next() {
            let key = self.condition.probe_key(&row);

            // Skip if Bloom filter rejects
            if let Some(ref bf) = bloom {
                if \!bf.might_contain(key) {
                    continue;
                }
            }

            for build_row in ht.lookup(key) {
                results.push(Row::join(build_row, &row));
            }
        }

        Ok(results)
    }

    fn probe_estimate(&self) -> usize {
        1_000_000 // placeholder; real impl uses statistics
    }
}
```

## Cost Model

**Bloom filter build:**
- Time: `distinct_keys * num_hashes * hash_cost` (~50 ns/key)
- Memory: `distinct_keys * 10 bits` (for 1% FPR)
- Example: 1M distinct keys -> 1.25 MB filter, ~50 ms build

**Probe filtering:**
- Per-row check: `num_hashes * bit_test` (~10-20 ns)
- True negative: O(1) rejection (no hash table probe needed)
- False positive: ~1% extra hash table probes (negligible)

**Net benefit:**
- Rows eliminated: `probe_rows * (1 - build_distinct / probe_domain)`
- Savings per eliminated row: hash table probe cost (~50-200 ns)
- Break-even: filter pays for itself after eliminating ~1000 rows

**When not beneficial:**
- Build side large (>50% of probe domain): filter passes most rows
- Probe side already filtered: little additional selectivity
- In-memory hash table fits in cache: probe cost already low

## Test Cases

```sql
-- Test 1: Selective join -> Bloom filter deployed
SELECT o.*, l.*
FROM orders o
JOIN lineitem l ON o.order_id = l.order_id
WHERE o.order_date = '2024-01-01';
-- ~2K orders on that date, 6M lineitems
-- Bloom filter on 2K keys eliminates 99.97% of lineitem reads

-- Test 2: Non-selective join -> filter skipped
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.customer_id;
-- All customers have orders: filter would pass everything
-- Decision: skip Bloom filter

-- Test 3: Multi-join filter propagation
SELECT * FROM fact
JOIN dim1 ON fact.d1_id = dim1.id AND dim1.category = 'A'
JOIN dim2 ON fact.d2_id = dim2.id AND dim2.region = 'US';
-- Bloom from dim1 (100 keys) pushed to fact scan
-- Bloom from dim2 (50 keys) also pushed to fact scan
-- Combined: eliminates 99.9%+ of fact rows

-- Test 4: Partition pruning via Bloom
SELECT * FROM partitioned_fact f
JOIN date_dim d ON f.date_key = d.key
WHERE d.year = 2024;
-- Bloom filter on 365 date keys
-- Eliminates entire partitions that don't match
```

## References

1. **Bloom, Burton H**. "Space/Time Trade-offs in Hash Coding with Allowable Errors." CACM 1970.
   - Original Bloom filter paper

2. **Oracle Documentation**. "Bloom Filter Pruning." Oracle 10g+.
   - Runtime Bloom filter deployment for join optimization

3. **Spark Documentation**. "Dynamic Partition Pruning." Spark 3.0+.
   - Bloom filter-based partition elimination at runtime

4. **Putze, Felix et al**. "Cache-, Hash- and Space-Efficient Bloom Filters." JEA 2009.
   - Cache-efficient Bloom filter implementations
