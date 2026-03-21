# Rule: Column-at-a-Time Hash Join

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-hash-join.rra`

## Metadata

- **ID:** `column-hash-join`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise
- **Tags:** execution, columnar, x100, hash-join, late-materialization, partitioning, cache-conscious
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, Marcin Zukowski, Stefan Manegold


# Column-at-a-Time Hash Join

## Description

Column-at-a-time hash join operates on column arrays rather than individual tuples. The build phase hashes the join key column of the inner relation and constructs a hash table mapping keys to row positions (not full rows). The probe phase hashes the join key column of the outer relation and looks up matching positions. Only after matching positions are determined are the payload columns gathered via late materialization. This minimizes data movement: during the hash table phase, only join key columns are accessed.

**Column hash join phases:**
1. **Build key scan**: Read only the join key column of the inner relation
2. **Hash computation**: Hash entire key column in a tight SIMD-friendly loop
3. **Hash table insert**: Insert (hash, position) pairs into hash table
4. **Probe key scan**: Read only the join key column of the outer relation
5. **Hash + probe**: Hash outer keys, probe hash table, collect matching position pairs
6. **Late gather**: Gather payload columns at matched positions from both sides

**Key characteristics:**
- **Late materialization**: Hash table stores positions, not rows
- **Column-oriented build**: Only join key column read during build
- **Bulk hash computation**: Hash entire column array, not per-tuple
- **Partitioned variant**: Radix-partition columns for cache-conscious join
- **Minimal data movement**: Payload columns only read at matching positions

**Partitioned (radix) hash join for cache-consciousness:**
- Radix-partition both inputs on hash bits
- Each partition pair fits in cache
- Join each partition pair independently
- Avoids cache-miss-per-probe problem of large hash tables

**Trade-offs:**
- Hash table size: stores positions (4-8 bytes each) not full rows
- Late gather has random access pattern (cache misses for payload)
- Partitioning adds CPU cost but reduces cache misses
- Multi-column join keys require composite hash computation

## Relational Algebra

```
ColumnHashJoin(R, S, R.a = S.b)
  Phase 1 (Build):
    keys_R = ColumnScan(R, [a])
    hashes_R = hash_column(keys_R)
    ht = build_hash_table(hashes_R, positions_R)

  Phase 2 (Probe):
    keys_S = ColumnScan(S, [b])
    hashes_S = hash_column(keys_S)
    (match_R, match_S) = probe_hash_table(ht, hashes_S, keys_R, keys_S)

  Phase 3 (Gather):
    output = gather_columns(R, match_R, payload_cols_R)
           + gather_columns(S, match_S, payload_cols_S)
```

## Implementation

```rust
/// Column-at-a-time hash join operator
pub struct ColumnHashJoin {
    join_key_left: ColumnId,
    join_key_right: ColumnId,
    /// Payload columns to output from each side
    payload_left: Vec<ColumnId>,
    payload_right: Vec<ColumnId>,
    /// Use partitioned (radix) join for large inputs
    use_partitioning: bool,
    /// Number of radix bits for partitioning
    radix_bits: u32,
}

/// Position-based hash table (stores row positions, not data)
struct PositionHashTable {
    /// Hash table: hash -> chain of positions
    buckets: Vec<u32>,      // head of chain per bucket
    next: Vec<u32>,         // next pointer per entry
    positions: Vec<u32>,    // row position per entry
    hash_values: Vec<u64>,  // stored hash per entry
    mask: u64,
    num_entries: usize,
}

impl PositionHashTable {
    fn new(capacity: usize) -> Self {
        let num_buckets = capacity.next_power_of_two();
        Self {
            buckets: vec![u32::MAX; num_buckets],
            next: Vec::with_capacity(capacity),
            positions: Vec::with_capacity(capacity),
            hash_values: Vec::with_capacity(capacity),
            mask: (num_buckets - 1) as u64,
            num_entries: 0,
        }
    }

    /// Bulk insert: process entire column of hashes
    fn build_from_column(
        &mut self,
        hashes: &[u64],
        num_rows: usize,
    ) {
        for pos in 0..num_rows {
            let hash = hashes[pos];
            let bucket = (hash & self.mask) as usize;

            // Chain: new entry points to old head
            self.next.push(self.buckets[bucket]);
            self.buckets[bucket] = self.num_entries as u32;
            self.positions.push(pos as u32);
            self.hash_values.push(hash);
            self.num_entries += 1;
        }
    }

    /// Bulk probe: process entire column of hashes
    fn probe_column(
        &self,
        probe_hashes: &[u64],
        probe_keys: &ColumnArray,
        build_keys: &ColumnArray,
        num_probe: usize,
    ) -> (Vec<u32>, Vec<u32>) {
        let mut match_build = Vec::new();
        let mut match_probe = Vec::new();

        for probe_pos in 0..num_probe {
            let hash = probe_hashes[probe_pos];
            let mut entry = self.buckets[
                (hash & self.mask) as usize
            ];

            while entry != u32::MAX {
                let idx = entry as usize;
                if self.hash_values[idx] == hash {
                    let build_pos = self.positions[idx];
                    // Key equality check
                    if keys_equal(
                        build_keys, build_pos as usize,
                        probe_keys, probe_pos,
                    ) {
                        match_build.push(build_pos);
                        match_probe.push(probe_pos as u32);
                    }
                }
                entry = self.next[idx];
            }
        }

        (match_build, match_probe)
    }
}

impl ColumnHashJoin {
    /// Execute column-at-a-time hash join
    pub fn execute(
        &self,
        left_columns: &[ColumnArray],
        right_columns: &[ColumnArray],
    ) -> Vec<ColumnArray> {
        let left_key = &left_columns[self.join_key_left];
        let right_key = &right_columns[self.join_key_right];

        if self.use_partitioning {
            return self.execute_partitioned(
                left_columns, right_columns,
            );
        }

        // Phase 1: Hash build key column
        let build_hashes = hash_column(left_key);

        // Phase 2: Build position hash table
        let mut ht = PositionHashTable::new(left_key.len);
        ht.build_from_column(&build_hashes, left_key.len);

        // Phase 3: Hash probe key column
        let probe_hashes = hash_column(right_key);

        // Phase 4: Probe and collect matching positions
        let (match_left, match_right) = ht.probe_column(
            &probe_hashes, right_key, left_key,
            right_key.len,
        );

        // Phase 5: Late gather -- materialize payload columns
        let left_sel = SelectionVector {
            source_len: left_key.len,
            positions: match_left,
        };
        let right_sel = SelectionVector {
            source_len: right_key.len,
            positions: match_right,
        };

        let mut output = Vec::new();
        for &col_id in &self.payload_left {
            output.push(gather(
                &left_columns[col_id], &left_sel,
            ));
        }
        for &col_id in &self.payload_right {
            output.push(gather(
                &right_columns[col_id], &right_sel,
            ));
        }

        output
    }

    /// Radix-partitioned hash join for cache-consciousness
    fn execute_partitioned(
        &self,
        left_columns: &[ColumnArray],
        right_columns: &[ColumnArray],
    ) -> Vec<ColumnArray> {
        let num_partitions = 1 << self.radix_bits;
        let left_key = &left_columns[self.join_key_left];
        let right_key = &right_columns[self.join_key_right];

        // Hash both key columns
        let left_hashes = hash_column(left_key);
        let right_hashes = hash_column(right_key);

        // Radix partition: scatter positions into partitions
        let left_parts = radix_partition(
            &left_hashes, left_key.len, self.radix_bits,
        );
        let right_parts = radix_partition(
            &right_hashes, right_key.len, self.radix_bits,
        );

        // Join each partition pair independently
        let mut all_match_left = Vec::new();
        let mut all_match_right = Vec::new();

        for p in 0..num_partitions {
            if left_parts[p].is_empty()
                || right_parts[p].is_empty()
            {
                continue;
            }

            // Build small hash table for this partition
            let mut ht = PositionHashTable::new(
                left_parts[p].len(),
            );
            let part_hashes: Vec<u64> = left_parts[p]
                .iter()
                .map(|&pos| left_hashes[pos as usize])
                .collect();
            ht.build_from_column(
                &part_hashes, left_parts[p].len(),
            );

            // Probe with right partition
            let probe_hashes: Vec<u64> = right_parts[p]
                .iter()
                .map(|&pos| right_hashes[pos as usize])
                .collect();
            let part_right_key = gather_positions(
                right_key, &right_parts[p],
            );
            let part_left_key = gather_positions(
                left_key, &left_parts[p],
            );

            let (ml, mr) = ht.probe_column(
                &probe_hashes, &part_right_key,
                &part_left_key, right_parts[p].len(),
            );

            // Map partition-local positions back to global
            for &local_pos in &ml {
                all_match_left.push(
                    left_parts[p][local_pos as usize],
                );
            }
            for &local_pos in &mr {
                all_match_right.push(
                    right_parts[p][local_pos as usize],
                );
            }
        }

        // Late gather payload columns
        let left_sel = SelectionVector {
            source_len: left_key.len,
            positions: all_match_left,
        };
        let right_sel = SelectionVector {
            source_len: right_key.len,
            positions: all_match_right,
        };

        let mut output = Vec::new();
        for &col_id in &self.payload_left {
            output.push(gather(
                &left_columns[col_id], &left_sel,
            ));
        }
        for &col_id in &self.payload_right {
            output.push(gather(
                &right_columns[col_id], &right_sel,
            ));
        }

        output
    }
}

/// Bulk hash computation on entire column array
fn hash_column(col: &ColumnArray) -> Vec<u64> {
    let data = col.data.as_i64_slice();
    let mut hashes = vec![0u64; col.len];

    // SIMD hashing: 4 hashes per cycle (CRC32 or multiply-shift)
    for i in 0..col.len {
        hashes[i] = multiply_shift_hash(data[i] as u64);
    }

    hashes
}
```

## Cost Model

**Build Phase:**
- Hash column: O(|R|) with ~1 ns/value (SIMD hash)
- Insert into hash table: O(|R|) with ~5 ns/entry
- Memory: |R| x (8 bytes hash + 4 bytes position + 4 bytes next) = 16 bytes/row

**Probe Phase:**
- Hash column: O(|S|) with ~1 ns/value
- Probe: O(|S|) x avg chain length
- Cache behavior depends on hash table size vs. cache

**Late Gather:**
- O(|matches| x num_payload_columns)
- Random access: ~10-50 ns per gather (L2/L3 cache miss)

**Partitioned vs. Non-partitioned:**

| Input Size | Non-partitioned | Partitioned (radix) |
|-----------|----------------|-------------------|
| HT fits L2 | ~5 ns/probe | ~8 ns/probe (overhead) |
| HT fits L3 | ~15 ns/probe | ~8 ns/probe |
| HT > L3 | ~60 ns/probe | ~10 ns/probe |

**Late materialization savings:**
- Without: hash table stores full rows (row_width bytes each)
- With: hash table stores positions (4 bytes each)
- Savings: `|R| x (row_width - 4)` bytes in hash table
- Additional: payload columns only read at match positions

## Test Cases

```sql
-- Test 1: Simple equi-join with late materialization
SELECT o.id, c.name, o.amount
FROM orders o JOIN customers c ON o.cust_id = c.id;
-- Build: hash customers.id column only (not name)
-- Probe: hash orders.cust_id column
-- Gather: customers.name at matched positions
--         orders.id, orders.amount at matched positions

-- Test 2: Large build side (partitioned)
SELECT * FROM lineitem l JOIN orders o ON l.orderkey = o.orderkey;
-- orders: 1.5M rows -> hash table ~24 MB (exceeds L2)
-- Use radix partitioning: 256 partitions, ~94 KB each (fits L2)
-- 3-5x faster than non-partitioned for this size

-- Test 3: Multi-column join key
SELECT * FROM t1 JOIN t2 ON t1.a = t2.a AND t1.b = t2.b;
-- Hash: combine hash(a) and hash(b) into composite hash
-- Equality check: compare both columns at matching positions

-- Test 4: Join with post-filter
SELECT o.id, p.name FROM orders o JOIN products p
  ON o.product_id = p.id WHERE o.amount > 100;
-- Optimization: filter orders first (reduce probe side)
-- Then join with filtered selection vector
-- Late gather only at positions passing both filter and join
```

## Comparison

| Property | Column Hash Join | Row Hash Join | Sort-Merge Join |
|----------|-----------------|---------------|-----------------|
| Build data | Key column only | Full rows | Sorted keys |
| HT memory | 16 bytes/row | row_width/row | None (sorted) |
| Probe access | Key + HT lookup | Key + HT lookup | Sequential merge |
| Payload access | Late gather (random) | Already in HT | Late gather |
| Cache behavior | Partitioned: good | Poor for large HT | Sequential: good |
| Duplicate keys | Chain traversal | Chain traversal | Sequential scan |
| Build cost | O(R) hash + insert | O(R) hash + insert | O(R log R) sort |

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
2. **Manegold, Stefan; Boncz, Peter; Nes, Niels**. "What Happens During a Join? Dissecting CPU and Memory Optimization Effects." VLDB 2000.
3. **Kim, Changkyu; Kaldewey, Tim; Lee, Victor; Sedlar, Eric; Nguyen, Anthony; Satish, Nadathur; Chhugani, Jatin; Di Blas, Andrea; Dubey, Pradeep**. "Sort vs. Hash Revisited: Fast Join Implementation on Modern Multi-Core CPUs." VLDB 2009.
4. **Balkesen, Cagri; Alonso, Gustavo; Teubner, Jens; Ozsu, M. Tamer**. "Multi-Core, Main-Memory Joins: Sort vs. Hash Revisited." VLDB 2014.
