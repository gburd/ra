# Rule: Column-at-a-Time Aggregation

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-aggregate.rra`

## Metadata

- **ID:** `column-aggregate`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise, vertica
- **Tags:** execution, columnar, x100, aggregate, group-by, hash-aggregate, simd
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, Marcin Zukowski


# Column-at-a-Time Aggregation

## Description

Column-at-a-time aggregation processes group-by keys and aggregate values as full column arrays. The group-by key column is hashed to assign each row to a group, and the aggregate value column is accumulated into per-group state. For queries without GROUP BY (scalar aggregation), the entire value column is reduced in a single tight loop, achieving near-memory-bandwidth throughput with SIMD. For grouped aggregation, the pattern is: hash key column, probe/insert into group hash table, update accumulators using the value column.

**Aggregation patterns:**
1. **Scalar (no GROUP BY)**: Reduce entire column to single value -- O(N) tight loop
2. **Low cardinality GROUP BY**: Dense group array, direct-index accumulators
3. **High cardinality GROUP BY**: Hash table mapping keys to accumulator slots
4. **Pre-sorted GROUP BY**: Sequential scan with group boundary detection
5. **Two-phase (parallel)**: Thread-local aggregation + global merge

**Key characteristics:**
- **Column-oriented accumulation**: Process value column in bulk, not per tuple
- **SIMD scalar reduction**: SUM via SIMD horizontal add (4-8 values/cycle)
- **Hash table for groups**: Column of hashes indexes into group hash table
- **Bulk hash computation**: Hash entire key column before probing
- **Prefetch-friendly**: Prefetch hash table slots while processing next batch

**Aggregation functions and column processing:**
- **SUM(col)**: SIMD reduce column array (horizontal add)
- **COUNT(*)**: Simply column length (or popcount of selection bitmap)
- **MIN/MAX(col)**: SIMD reduce with min/max instructions
- **AVG(col)**: SUM + COUNT, divide at finalization
- **COUNT(DISTINCT col)**: Hash set built from column values

**Trade-offs:**
- Hash table dominates cost for high-cardinality GROUP BY
- Low-cardinality groups allow array-based (no hashing) accumulation
- Scalar aggregation is memory-bandwidth bound (trivial CPU work)
- Column-at-a-time requires materializing full hash/group arrays

## Relational Algebra

```
ColumnAggregate([key_cols], [agg_funcs(val_cols)])
  Phase 1: hash_col = hash_column(key_cols)
  Phase 2: group_ids = probe_or_insert(hash_table, hash_col, key_cols)
  Phase 3: update_accumulators(group_ids, val_cols, agg_funcs)
  Phase 4: finalize(hash_table) -> output columns

ScalarAggregate([agg_funcs(val_cols)])
  = reduce(val_cols, agg_funcs) -> single row
```

## Implementation

```rust
/// Column-at-a-time aggregation operator
pub struct ColumnAggregate {
    group_keys: Vec<ColumnId>,
    agg_funcs: Vec<AggFunc>,
    /// Group hash table
    group_table: GroupHashTable,
}

pub struct GroupHashTable {
    /// Hash table: hash -> group_id
    buckets: Vec<u32>,
    mask: u64,
    /// Per-group state
    group_keys_stored: Vec<Vec<ScalarValue>>,
    accumulators: Vec<Vec<Accumulator>>,
    num_groups: usize,
}

pub enum Accumulator {
    SumI64(i64),
    SumF64(f64),
    Count(i64),
    MinI64(Option<i64>),
    MaxI64(Option<i64>),
    Avg { sum: f64, count: i64 },
    DistinctCount(HashSet<u64>),
}

impl ColumnAggregate {
    /// Scalar aggregation (no GROUP BY)
    pub fn aggregate_scalar(
        &self,
        columns: &[ColumnArray],
        sel: Option<&SelectionVector>,
    ) -> Vec<ScalarValue> {
        self.agg_funcs.iter().map(|func| {
            let col = &columns[func.column];
            match func.kind {
                AggKind::Sum => {
                    scalar_sum_column(col, sel)
                }
                AggKind::Count => {
                    let n = sel.map(|s| s.positions.len())
                        .unwrap_or(col.len);
                    ScalarValue::Int64(n as i64)
                }
                AggKind::Min => scalar_min_column(col, sel),
                AggKind::Max => scalar_max_column(col, sel),
                AggKind::Avg => {
                    let sum = scalar_sum_f64(col, sel);
                    let count = sel
                        .map(|s| s.positions.len())
                        .unwrap_or(col.len);
                    ScalarValue::Float64(
                        sum / count as f64,
                    )
                }
            }
        }).collect()
    }

    /// Grouped aggregation
    pub fn aggregate_grouped(
        &mut self,
        columns: &[ColumnArray],
        sel: Option<&SelectionVector>,
    ) {
        let num_rows = sel.map(|s| s.positions.len())
            .unwrap_or(columns[0].len);

        // Phase 1: Hash group key columns
        let hashes = hash_group_keys(
            &self.group_keys, columns, sel,
        );

        // Phase 2: Probe/insert group hash table
        //   Returns group_id for each input row
        let group_ids = self.group_table.probe_or_insert(
            &hashes,
            &self.group_keys,
            columns,
            sel,
            num_rows,
        );

        // Phase 3: Update accumulators per group
        for (func_idx, func) in
            self.agg_funcs.iter().enumerate()
        {
            let val_col = &columns[func.column];
            update_accumulators_bulk(
                &mut self.group_table.accumulators,
                func_idx,
                &func.kind,
                val_col,
                &group_ids,
                sel,
                num_rows,
            );
        }
    }

    /// Finalize: produce output columns
    pub fn finalize(&self) -> Vec<ColumnArray> {
        let num_groups = self.group_table.num_groups;
        let mut output = Vec::new();

        // Output group key columns
        for key_idx in 0..self.group_keys.len() {
            let mut data = AlignedBuffer::new(
                num_groups * 8,
            );
            for g in 0..num_groups {
                write_scalar(
                    &mut data, g,
                    &self.group_table.group_keys_stored[g]
                        [key_idx],
                );
            }
            output.push(ColumnArray {
                col_id: self.group_keys[key_idx],
                data_type: DataType::Int64,
                data,
                null_bitmap: None,
                len: num_groups,
            });
        }

        // Output aggregate result columns
        for func_idx in 0..self.agg_funcs.len() {
            let mut data = AlignedBuffer::new(
                num_groups * 8,
            );
            for g in 0..num_groups {
                let val = self.group_table
                    .accumulators[g][func_idx]
                    .finalize();
                write_scalar(&mut data, g, &val);
            }
            output.push(ColumnArray {
                col_id: 0,
                data_type: DataType::Float64,
                data,
                null_bitmap: None,
                len: num_groups,
            });
        }

        output
    }
}

/// SIMD scalar SUM for i64 column
fn scalar_sum_column(
    col: &ColumnArray,
    sel: Option<&SelectionVector>,
) -> ScalarValue {
    let data = col.data.as_i64_slice();

    match sel {
        None => {
            // Full column SIMD reduction
            let mut sum: i64 = 0;

            #[cfg(target_arch = "x86_64")]
            {
                let mut acc = unsafe {
                    _mm256_setzero_si256()
                };
                let chunks = col.len / 4;
                for i in 0..chunks {
                    let v = unsafe {
                        _mm256_loadu_si256(
                            data[i * 4..].as_ptr()
                                as *const _,
                        )
                    };
                    acc = unsafe {
                        _mm256_add_epi64(acc, v)
                    };
                }
                // Horizontal sum of 4 lanes
                let arr = unsafe {
                    std::mem::transmute::<_, [i64; 4]>(acc)
                };
                sum = arr[0] + arr[1] + arr[2] + arr[3];
                // Scalar remainder
                for i in (chunks * 4)..col.len {
                    sum += data[i];
                }
            }

            ScalarValue::Int64(sum)
        }
        Some(sv) => {
            // Gather-sum at selected positions
            let mut sum: i64 = 0;
            for &pos in &sv.positions {
                sum += data[pos as usize];
            }
            ScalarValue::Int64(sum)
        }
    }
}

/// Bulk accumulator update (column-at-a-time)
fn update_accumulators_bulk(
    accumulators: &mut Vec<Vec<Accumulator>>,
    func_idx: usize,
    kind: &AggKind,
    val_col: &ColumnArray,
    group_ids: &[u32],
    sel: Option<&SelectionVector>,
    num_rows: usize,
) {
    let data = val_col.data.as_i64_slice();

    for i in 0..num_rows {
        let pos = sel.map(|s| s.positions[i] as usize)
            .unwrap_or(i);
        let group = group_ids[i] as usize;
        let val = data[pos];

        match &mut accumulators[group][func_idx] {
            Accumulator::SumI64(sum) => *sum += val,
            Accumulator::Count(count) => *count += 1,
            Accumulator::MinI64(min) => {
                *min = Some(min.map_or(
                    val, |m: i64| m.min(val),
                ));
            }
            Accumulator::MaxI64(max) => {
                *max = Some(max.map_or(
                    val, |m: i64| m.max(val),
                ));
            }
            Accumulator::Avg { sum, count } => {
                *sum += val as f64;
                *count += 1;
            }
            _ => {}
        }
    }
}

/// Low-cardinality optimization: direct-indexed accumulation
fn aggregate_low_cardinality(
    key_col: &ColumnArray,
    val_col: &ColumnArray,
    max_key: usize,
) -> Vec<i64> {
    let keys = key_col.data.as_i32_slice();
    let vals = val_col.data.as_i64_slice();
    let mut accum = vec![0i64; max_key + 1];

    for i in 0..key_col.len {
        accum[keys[i] as usize] += vals[i];
    }

    accum
}
```

## Cost Model

**Scalar Aggregation (no GROUP BY):**
- SUM/COUNT: ~0.25 ns/value (SIMD, memory-bandwidth bound)
- MIN/MAX: ~0.25 ns/value (SIMD min/max instructions)
- AVG: ~0.5 ns/value (SUM + COUNT)
- Throughput: ~16 GB/s for i64 columns (DDR4 bandwidth limited)

**Grouped Aggregation:**
- Hash computation: ~1 ns/row
- HT probe + insert: ~5-60 ns/row (depends on HT size vs. cache)
- Accumulator update: ~1 ns/row (single add)
- Bottleneck: hash table probing (random access)

**By Group Cardinality:**

| Groups | Strategy | Probe Cost | Total (1M rows) |
|--------|----------|-----------|-----------------|
| 1-100 | Dense array | ~1 ns/row | ~1 ms |
| 100-10K | Small HT (L1/L2) | ~5 ns/row | ~5 ms |
| 10K-1M | Large HT (L3) | ~15 ns/row | ~15 ms |
| >1M | HT > L3 | ~60 ns/row | ~60 ms |

**Memory:**
- Scalar: O(1) per aggregate (single accumulator)
- Grouped: O(|groups| x num_aggregates x accumulator_size)
- Hash table overhead: ~24 bytes per group entry

## Test Cases

```sql
-- Test 1: Scalar SUM (SIMD reduction)
SELECT SUM(amount) FROM transactions;
-- SIMD: 4 i64 adds per cycle, 1M rows in ~0.25 ms
-- Memory-bandwidth bound: 8 MB column at ~32 GB/s

-- Test 2: Low cardinality GROUP BY
SELECT status, COUNT(*), SUM(amount)
FROM orders GROUP BY status;
-- 5 status values: use direct-indexed array
-- No hash table needed: array[status] += amount
-- Cost: ~1 ns/row, 1M rows in ~1 ms

-- Test 3: High cardinality GROUP BY
SELECT customer_id, SUM(amount), AVG(quantity)
FROM orders GROUP BY customer_id;
-- 100K customers: hash table ~2.4 MB (fits L3)
-- Probe cost: ~15 ns/row, 10M rows in ~150 ms
-- Prefetching hash table slots reduces to ~100 ms

-- Test 4: Multi-column GROUP BY
SELECT region, product_type, SUM(revenue)
FROM sales GROUP BY region, product_type;
-- Composite hash of (region, product_type)
-- Groups: region x product_type combinations
-- Hash + probe: ~20 ns/row
```

## Comparison

| Property | Column Aggregate | Row Aggregate | Compiled Aggregate |
|----------|-----------------|---------------|-------------------|
| SIMD scalar | Direct (4-8/cycle) | Not possible | Possible (fused) |
| Group lookup | Bulk hash + probe | Per-tuple hash | Fused in loop |
| Prefetching | Between batches | Per-tuple | In generated code |
| Low cardinality | Direct-indexed | Hash table | Switch/jump table |
| Memory access | Column sequential | Row random | Register + HT |
| Interpretation | 1 call per column | 1 call per tuple | None |

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
2. **Zukowski, Marcin; Heman, Sandor; Nes, Niels; Boncz, Peter A.**. "Super-Scalar RAM-CPU Cache Compression." ICDE 2006.
3. **Ye, Yuhao; Ross, Kenneth A.; Vesdapunt, Norases**. "Scalable Aggregation on Multicore Processors." DaMoN 2011.
4. **Polychroniou, Orestis; Raghavan, Arun; Ross, Kenneth A.**. "Rethinking SIMD Vectorization for In-Memory Databases." SIGMOD 2015.
