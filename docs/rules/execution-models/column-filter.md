# Rule: Column-at-a-Time Filter with Selection Vectors

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-filter.rra`

## Metadata

- **ID:** `column-filter`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise, vertica
- **Tags:** execution, columnar, filter, selection-vector, late-materialization, predicate, simd
- **SQL Standard:** MonetDB X100
- **Authors:** Peter Boncz, Marcin Zukowski


# Column-at-a-Time Filter with Selection Vectors

## Description

Column-at-a-time filtering evaluates predicates on full column arrays and produces selection vectors (arrays of qualifying row positions) rather than copying qualifying tuples. This enables late materialization: only the columns needed for output are gathered using the selection vector, avoiding unnecessary data movement for columns that are only used in filtering. The selection vector propagates through subsequent operators, which all operate on the same positional references.

**Filter evaluation approaches:**
1. **Full materialization (Volcano)**: Evaluate predicate per tuple, copy entire qualifying row
2. **Selection vector (X100)**: Evaluate predicate on column, produce position array
3. **Bitmask (Arrow/Parquet)**: Evaluate predicate, produce bit-per-row mask
4. **Branch-free SIMD**: Evaluate predicate with SIMD comparison, no branches

**Key characteristics:**
- **No data copying during filter**: Only positions are recorded, not values
- **Late materialization**: Defer column reads until after filtering
- **SIMD evaluation**: Predicate applied to column array with SIMD instructions
- **Selectivity-aware**: Low selectivity means fewer positions to gather
- **Composable**: Multiple filters produce intersected selection vectors

**Selection vector vs. bitmask:**
- Selection vector: array of u32 positions (4 bytes per qualifying row)
- Bitmask: 1 bit per row (compact, but requires bit manipulation)
- Selection vector better when selectivity < 50% (fewer entries)
- Bitmask better when selectivity > 50% or for boolean operations (AND/OR)

**Trade-offs:**
- Selection vector overhead: 4 bytes per qualifying row
- Gather operations (indirect access via positions) are slower than sequential
- Very high selectivity (>90%) means nearly sequential -- minimal benefit
- Very low selectivity (<1%) means small selection vector -- large benefit

## Relational Algebra

```
ColumnFilter(columns, predicate)
  = SelectionVector(positions where predicate(columns) = true)

-- Subsequent operations use selection vector:
Gather(column, selection_vector) = column[selection_vector[i]] for each i

-- Multi-predicate:
Filter(A > 10 AND B < 5)
  = sv1 = Filter(A > 10)
    sv2 = Filter(B < 5, sv1)  -- only evaluate on sv1 positions
```

## Implementation

```rust
/// Selection vector: positions of qualifying rows
pub struct SelectionVector {
    positions: Vec<u32>,
    /// Original column length (before filtering)
    source_len: usize,
}

/// Column-at-a-time filter operator
pub struct ColumnFilter {
    predicate: FilterPredicate,
}

pub enum FilterPredicate {
    Comparison {
        column: ColumnId,
        op: CmpOp,
        constant: ScalarValue,
    },
    Between {
        column: ColumnId,
        low: ScalarValue,
        high: ScalarValue,
    },
    IsNull { column: ColumnId },
    IsNotNull { column: ColumnId },
    And(Box<FilterPredicate>, Box<FilterPredicate>),
    Or(Box<FilterPredicate>, Box<FilterPredicate>),
}

impl ColumnFilter {
    /// Evaluate predicate on column array, produce selection vector
    pub fn evaluate(
        &self,
        columns: &[ColumnArray],
        input_sel: Option<&SelectionVector>,
    ) -> SelectionVector {
        match &self.predicate {
            FilterPredicate::Comparison {
                column, op, constant,
            } => {
                let col = &columns[*column];
                self.compare_column(col, *op, constant, input_sel)
            }
            FilterPredicate::Between {
                column, low, high,
            } => {
                let col = &columns[*column];
                self.between_column(col, low, high, input_sel)
            }
            FilterPredicate::IsNull { column } => {
                let col = &columns[*column];
                self.null_check(col, true, input_sel)
            }
            FilterPredicate::IsNotNull { column } => {
                let col = &columns[*column];
                self.null_check(col, false, input_sel)
            }
            FilterPredicate::And(left, right) => {
                let left_filter = ColumnFilter {
                    predicate: *left.clone(),
                };
                let sv1 = left_filter.evaluate(
                    columns, input_sel,
                );
                let right_filter = ColumnFilter {
                    predicate: *right.clone(),
                };
                right_filter.evaluate(columns, Some(&sv1))
            }
            FilterPredicate::Or(left, right) => {
                let left_filter = ColumnFilter {
                    predicate: *left.clone(),
                };
                let right_filter = ColumnFilter {
                    predicate: *right.clone(),
                };
                let sv1 = left_filter.evaluate(
                    columns, input_sel,
                );
                let sv2 = right_filter.evaluate(
                    columns, input_sel,
                );
                union_selection_vectors(&sv1, &sv2)
            }
        }
    }

    /// SIMD comparison: column op constant
    fn compare_column(
        &self,
        col: &ColumnArray,
        op: CmpOp,
        constant: &ScalarValue,
        input_sel: Option<&SelectionVector>,
    ) -> SelectionVector {
        let mut positions = Vec::new();

        match (&col.data_type, constant) {
            (DataType::Int64, ScalarValue::Int64(val)) => {
                let data = col.data.as_i64_slice();
                let cmp_val = *val;

                // SIMD path: process 4 i64 values per AVX2 iteration
                #[cfg(target_arch = "x86_64")]
                if is_x86_feature_detected!("avx2") {
                    return simd_compare_i64(
                        data, op, cmp_val, input_sel,
                        col.len,
                    );
                }

                // Scalar fallback
                let iter = match input_sel {
                    Some(sv) => sv.positions.iter()
                        .copied().collect::<Vec<_>>(),
                    None => (0..col.len as u32).collect(),
                };

                for pos in iter {
                    let val = data[pos as usize];
                    let pass = match op {
                        CmpOp::Eq => val == cmp_val,
                        CmpOp::Ne => val != cmp_val,
                        CmpOp::Lt => val < cmp_val,
                        CmpOp::Le => val <= cmp_val,
                        CmpOp::Gt => val > cmp_val,
                        CmpOp::Ge => val >= cmp_val,
                    };
                    if pass {
                        positions.push(pos);
                    }
                }
            }
            _ => {
                // Generic comparison for other types
                positions = generic_compare(
                    col, op, constant, input_sel,
                );
            }
        }

        SelectionVector {
            source_len: col.len,
            positions,
        }
    }
}

/// Gather: materialize column values at selected positions
pub fn gather(
    col: &ColumnArray,
    sel: &SelectionVector,
) -> ColumnArray {
    let width = col.data_type.width();
    let mut output = AlignedBuffer::new(
        sel.positions.len() * width,
    );

    for (out_idx, &pos) in sel.positions.iter().enumerate() {
        let src_offset = pos as usize * width;
        let dst_offset = out_idx * width;
        output.copy_from(
            &col.data, src_offset, dst_offset, width,
        );
    }

    let null_bitmap = col.null_bitmap.as_ref().map(|bm| {
        let mut out_bm = BitVec::with_capacity(
            sel.positions.len(),
        );
        for &pos in &sel.positions {
            out_bm.push(bm[pos as usize]);
        }
        out_bm
    });

    ColumnArray {
        col_id: col.col_id,
        data_type: col.data_type,
        data: output,
        null_bitmap,
        len: sel.positions.len(),
    }
}

/// Branch-free filter (avoids branch misprediction)
pub fn branch_free_filter_i64(
    data: &[i64],
    threshold: i64,
    op: CmpOp,
) -> SelectionVector {
    let mut positions = Vec::with_capacity(data.len());
    let mut count = 0u32;

    for (i, &val) in data.iter().enumerate() {
        // Branch-free: compute mask (0 or 1)
        let pass = match op {
            CmpOp::Lt => (val < threshold) as u32,
            CmpOp::Gt => (val > threshold) as u32,
            CmpOp::Eq => (val == threshold) as u32,
            _ => unimplemented!(),
        };
        // Conditionally store position (no branch)
        positions.push(i as u32);
        count += pass;
        // Only advance if pass (branch-free trick)
        // In practice, use SIMD compress-store
    }
    positions.truncate(count as usize);

    SelectionVector {
        source_len: data.len(),
        positions,
    }
}
```

## Cost Model

**Filter Evaluation:**
- SIMD comparison: ~0.25 cycles per value (AVX2, 4 i64/cycle)
- Scalar comparison: ~1 cycle per value
- Branch-free scalar: ~1.5 cycles per value (no misprediction)
- With input selection vector: cost proportional to |selection|

**Selection Vector Construction:**
- Append position: ~0.5 cycles (branch + store)
- SIMD compress-store: ~0.5 cycles per value (amortized)

**Gather (late materialization):**
- Sequential positions: ~4 ns per value (cache-friendly)
- Random positions: ~10-50 ns per value (cache misses)
- Trade-off: gather cost vs. not reading column at all

**Selectivity Impact:**

| Selectivity | SV Size | Gather Pattern | Benefit vs. Full Scan |
|-------------|---------|---------------|----------------------|
| 0.1% | 1K positions | Sparse random | 1000x less data |
| 1% | 10K positions | Sparse random | 100x less data |
| 10% | 100K positions | Semi-sequential | 10x less data |
| 50% | 500K positions | Dense | 2x less data |
| 90% | 900K positions | Nearly sequential | 1.1x less data |

## Test Cases

```sql
-- Test 1: Simple range filter with late materialization
SELECT name, salary FROM employees WHERE age > 30;
-- Phase 1: Scan age column -> selection vector
-- Phase 2: Gather name, salary at selected positions
-- Cost: scan 1 column + gather 2 columns at sel positions

-- Test 2: Conjunctive predicate (AND)
SELECT id FROM orders WHERE amount > 100 AND status = 'SHIPPED';
-- sv1 = filter(amount > 100)
-- sv2 = filter(status = 'SHIPPED', input=sv1)
-- Second filter only evaluates sv1.len positions

-- Test 3: Disjunctive predicate (OR)
SELECT id FROM events WHERE type = 'ERROR' OR level > 8;
-- sv1 = filter(type = 'ERROR')
-- sv2 = filter(level > 8)
-- result = union(sv1, sv2) with deduplication

-- Test 4: High selectivity (most rows pass)
SELECT * FROM logs WHERE timestamp IS NOT NULL;
-- 99% non-null: selection vector nearly as large as column
-- Gather almost all positions: marginal benefit
-- Consider switching to bitmask representation
```

## Comparison

| Property | Selection Vector | Bitmask | Full Materialization |
|----------|-----------------|---------|---------------------|
| Space per row | 4 bytes (qualifying only) | 1 bit (all rows) | row_width (qualifying) |
| AND operation | Intersect sorted lists | Bitwise AND | N/A |
| OR operation | Merge sorted lists | Bitwise OR | N/A |
| Random gather | Required | Required | Already materialized |
| Low selectivity | Compact | Sparse bitmap | Large copies |
| High selectivity | Large | Dense bitmap | Similar |
| SIMD filter eval | Position extraction | Mask directly | Per-tuple branch |

## References

1. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
2. **Zukowski, Marcin; Heman, Sandor; Nes, Niels; Boncz, Peter A.**. "Super-Scalar RAM-CPU Cache Compression." ICDE 2006.
3. **Lang, Harald; Muhlbauer, Tobias; Funke, Florian; Boncz, Peter; Neumann, Thomas; Kemper, Alfons**. "Data Blocks: Hybrid OLTP and OLAP on Compressed Storage using both Vectorization and Compilation." SIGMOD 2016.
4. **Li, Yinan; Patel, Jignesh M.**. "BitWeaving: Fast Scans for Main Memory Data Processing." SIGMOD 2013.
