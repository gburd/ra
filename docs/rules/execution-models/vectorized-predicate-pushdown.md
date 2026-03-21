# Rule: Vectorized Predicate Pushdown into Scan

**Category:** execution-models
**File:** `rules/execution-models/vectorized/vectorized-predicate-pushdown.rra`

## Metadata

- **ID:** `vectorized-predicate-pushdown`
- **Version:** 1.0.0
- **Databases:** DuckDB, ClickHouse, Velox, DataFusion
- **Tags:** execution, vectorized, pushdown, filter, scan, zone-maps, simd
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz


# Vectorized Predicate Pushdown into Scan

## Description

Predicate pushdown in vectorized execution moves filter predicates into the scan operator, evaluating them during batch construction rather than as a separate downstream operator. This enables three optimizations: (1) zone map / min-max index pruning skips entire segments, (2) SIMD predicate evaluation builds selection vectors during scan, and (3) late materialization reads only columns needed for surviving rows.

**Pushdown levels:**
1. **Segment pruning**: Skip entire data segments using zone maps (min/max)
2. **Batch filtering**: Evaluate predicate on batch, produce selection vector
3. **Late materialization**: Read remaining columns only for selected rows
4. **Compression-aware**: Evaluate on compressed data when possible

**Key characteristics:**
- **Early elimination**: Filter before materializing full rows
- **Bandwidth reduction**: Skip irrelevant data at the storage level
- **Selection vector propagation**: Downstream operators respect selection
- **Multi-predicate fusion**: Evaluate multiple predicates in one pass
- **Zone map integration**: O(1) segment skip decisions

**Trade-offs:**
- Zone maps require min/max metadata per segment
- Late materialization adds random access for qualifying rows
- Not beneficial when selectivity is near 100%
- Complex predicates may not be pushable (subqueries, UDFs)

## Relational Algebra

```
Before pushdown:
  Filter(pred, VectorizedScan(table, all_columns))

After pushdown:
  VectorizedScanWithFilter(table, pred, projected_columns)

Execution:
  fn next_batch() -> Batch:
    loop:
      segment = next_segment()

      // Level 1: Zone map pruning
      if !zone_map_may_match(segment, pred):
        skip_segment(segment)
        continue

      // Level 2: Read filter columns only
      filter_batch = read_columns(segment, pred.columns)

      // Level 3: SIMD predicate evaluation
      selection = simd_eval_predicate(pred, filter_batch)

      if selection.count() == 0:
        continue  // Skip entirely

      // Level 4: Late materialization of remaining columns
      full_batch = read_remaining_columns(segment, selection)
      full_batch.selection = selection

      return full_batch
```

## Implementation

```rust
use ra_core::algebra::RelExpr;

/// Vectorized scan with integrated predicate pushdown
pub struct VectorizedScanWithPushdown {
    table: String,
    predicates: Vec<Expr>,
    output_columns: Vec<ColumnId>,
    segment_cursor: SegmentCursor,
    batch_size: usize,
}

impl VectorizedScanWithPushdown {
    pub fn next_batch(&mut self) -> Result<Option<Batch>> {
        loop {
            let segment = match self.segment_cursor.next() {
                Some(s) => s,
                None => return Ok(None),
            };

            // Level 1: Zone map pruning
            if !self.zone_map_check(&segment)? {
                continue; // Skip entire segment
            }

            // Level 2: Read only columns needed for predicates
            let filter_columns = self.predicate_column_ids();
            let filter_batch = segment.read_columns(
                &filter_columns, self.batch_size,
            )?;

            if filter_batch.size == 0 {
                continue;
            }

            // Level 3: SIMD predicate evaluation
            let mut selection = Bitset::all_set(filter_batch.size);
            for pred in &self.predicates {
                let pred_result = simd_eval_predicate(
                    pred, &filter_batch,
                )?;
                selection = selection.bitand(&pred_result);

                // Short-circuit: if nothing passes, skip rest
                if selection.count() == 0 {
                    break;
                }
            }

            if selection.count() == 0 {
                continue; // No rows pass all predicates
            }

            // Level 4: Late materialization
            let remaining_columns: Vec<_> = self.output_columns
                .iter()
                .filter(|c| !filter_columns.contains(c))
                .cloned()
                .collect();

            let mut batch = if remaining_columns.is_empty() {
                // All output columns already read for filter
                filter_batch
            } else {
                // Read remaining columns only for selected rows
                let extra = segment.read_columns_selected(
                    &remaining_columns,
                    &selection,
                )?;
                filter_batch.merge_columns(extra)
            };

            batch.selection = Some(selection.to_indices());
            return Ok(Some(batch));
        }
    }

    /// Zone map check: can this segment contain matching rows?
    fn zone_map_check(&self, segment: &Segment) -> Result<bool> {
        for pred in &self.predicates {
            match pred {
                Expr::Compare { op: Op::Gt, col, value } => {
                    let max = segment.zone_map(*col).max;
                    if max <= *value {
                        return Ok(false); // All values <= threshold
                    }
                }
                Expr::Compare { op: Op::Lt, col, value } => {
                    let min = segment.zone_map(*col).min;
                    if min >= *value {
                        return Ok(false);
                    }
                }
                Expr::Compare { op: Op::Eq, col, value } => {
                    let zm = segment.zone_map(*col);
                    if *value < zm.min || *value > zm.max {
                        return Ok(false);
                    }
                }
                _ => {} // Cannot prune for complex predicates
            }
        }
        Ok(true)
    }
}

/// Multi-predicate SIMD fusion
pub fn fused_predicate_eval(
    predicates: &[Expr],
    batch: &Batch,
) -> Result<Bitset> {
    let mut result = Bitset::all_set(batch.size);

    // Sort predicates by estimated selectivity (most selective first)
    let mut sorted: Vec<_> = predicates.iter().collect();
    sorted.sort_by(|a, b| {
        a.estimated_selectivity()
            .partial_cmp(&b.estimated_selectivity())
            .unwrap()
    });

    for pred in sorted {
        let pred_bits = simd_eval_predicate(pred, batch)?;
        result = result.bitand(&pred_bits);

        // Early termination if no rows survive
        if result.count() == 0 {
            break;
        }
    }

    Ok(result)
}

/// Cost model for predicate pushdown
pub fn pushdown_cost(
    total_rows: f64,
    total_segments: usize,
    selectivity: f64,
    zone_map_skip_rate: f64,
) -> f64 {
    // Segments actually scanned
    let scanned_segments = total_segments as f64
        * (1.0 - zone_map_skip_rate);

    // Rows in scanned segments
    let scanned_rows = total_rows * (1.0 - zone_map_skip_rate);

    // Filter evaluation cost (SIMD)
    let filter_cost = scanned_rows * 0.000001;

    // Late materialization: read remaining columns for survivors
    let surviving_rows = scanned_rows * selectivity;
    let late_mat_cost = surviving_rows * 0.00001;

    // Zone map checks (O(1) per segment)
    let zone_check_cost = total_segments as f64 * 0.0001;

    zone_check_cost + filter_cost + late_mat_cost
}
```

## Cost Model

**Zone Map Pruning:**
- Check cost: O(1) per segment (compare min/max)
- Skip rate: depends on data distribution and predicate
- Sorted column: up to 99%+ skip rate
- Random distribution: ~selectivity skip rate
- Savings: skip_rate x segment_scan_cost

**SIMD Predicate Evaluation:**
- Single predicate: ~1 ns per row (SIMD comparison)
- Multiple predicates: fused with short-circuit
- Selection vector construction: bitwise AND of predicate results
- Total: `scanned_rows x num_predicates x 1ns`

**Late Materialization:**
- Read remaining columns only for surviving rows
- Best case (low selectivity): read 1% of remaining column data
- Worst case (high selectivity): same as reading all
- Break-even: beneficial when selectivity < 50%

**Combined Savings (typical TPC-H Q6):**
- Zone map pruning: skip ~50% of segments
- SIMD filtering: evaluate on remaining 50%
- Late materialization: read 4% of non-filter columns
- Total I/O reduction: ~10-20x

## Test Cases

```sql
-- Test 1: Zone map pruning on sorted column
SELECT * FROM events WHERE timestamp > '2024-06-01';
-- timestamp is sorted: zone maps skip early segments
-- Expected: ~50% segments pruned by zone map

-- Test 2: Multi-predicate fusion
SELECT l_orderkey
FROM lineitem
WHERE l_shipdate BETWEEN '1994-01-01' AND '1995-01-01'
  AND l_discount BETWEEN 0.05 AND 0.07
  AND l_quantity < 24;
-- Expected: Three SIMD predicates fused
-- Most selective predicate evaluated first

-- Test 3: Late materialization benefit
SELECT l_extendedprice, l_discount
FROM lineitem
WHERE l_shipdate > '1998-09-01';
-- Expected: Read l_shipdate for filter, then only
-- read l_extendedprice + l_discount for ~3% surviving rows

-- Test 4: No pushdown benefit (high selectivity)
SELECT * FROM small_table WHERE active = true;
-- 90% of rows are active: pushdown overhead not justified
-- Expected: Full scan is cheaper than late materialization
```

## Comparison with Other Models

| Aspect | Vectorized Pushdown | Volcano Filter | Push-Based |
|--------|-------------------|---------------|------------|
| Zone map pruning | Yes (segment level) | No | No |
| Late materialization | Yes | No (full tuple) | No |
| SIMD filter | Batch evaluation | Per-tuple | Compiled inline |
| Multi-predicate | Fused bitwise AND | Sequential check | Inline AND |

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
   - Predicate evaluation in vectorized execution

2. **Abadi, Daniel J. et al**. "Materialization Strategies in a Column-Oriented DBMS." ICDE 2007.
   - Early vs late materialization in columnar systems

3. **Raasveldt, Mark; Muhleisen, Hannes**. "DuckDB: an Embeddable Analytical Database." SIGMOD 2019.
   - Zone maps and predicate pushdown in DuckDB

4. **Sun, Liwen et al**. "Fine-grained Partitioning for Aggressive Data Skipping." SIGMOD 2014.
   - Advanced zone map and data skipping techniques
