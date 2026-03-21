# Rule: Column-at-a-Time Late Materialization

**Category:** execution-models
**File:** `rules/execution-models/column-at-a-time/column-materialization.rra`

## Metadata

- **ID:** `column-materialization`
- **Version:** 1.0.0
- **Databases:** MonetDB, ClickHouse, DuckDB, vectorwise, vertica, c-store
- **Tags:** execution, columnar, x100, late-materialization, early-materialization, row-reconstruction, position-list
- **SQL Standard:** MonetDB X100
- **Authors:** Daniel Abadi, Peter Boncz, Marcin Zukowski


# Column-at-a-Time Late Materialization

## Description

Late materialization is the strategy of deferring row reconstruction (stitching columns back into rows) until as late as possible in query execution. Instead of materializing full rows after scanning, column-at-a-time execution passes position lists (selection vectors) and individual column arrays through the query plan. Only at the final output stage (or when an operator requires full rows, such as a network send) are the qualifying columns gathered at the selected positions to form result rows.

**Materialization strategies:**
1. **Early materialization**: Reconstruct rows immediately after scan, process as rows
2. **Late materialization**: Pass positions + column refs, reconstruct at output
3. **Hybrid**: Materialize frequently-accessed column groups, keep others late
4. **Invisible join** (Abadi): Push position lists through joins before any materialization

**Late materialization benefit analysis:**
- Only columns needed for predicates are read during filtering
- Only qualifying positions are used to gather payload columns
- If selectivity is low (few rows qualify), large payload columns are barely touched
- Intermediate operators (filter, join probe) work on narrow data (keys + positions)

**Key characteristics:**
- **Position-based references**: Operators pass position lists, not materialized rows
- **Column independence**: Each column accessed independently at selected positions
- **Selectivity leverage**: Low selectivity means most column data never read
- **Deferred gather**: Random access at gather time, but only for qualifying rows
- **Operator requirements**: Some operators (sort, network send) force materialization

**Materialization points (where rows must be reconstructed):**
- **ORDER BY**: Sort requires comparing full sort keys (or position-based sort)
- **Hash join build**: Must store full rows in hash table (or use position-based HT)
- **Network send**: Client expects rows, not column positions
- **User-defined functions**: UDFs typically expect row-oriented input
- **Nested loop join**: Inner relation accessed per-outer-row

**Trade-offs:**
- Gather (random column access) has cache misses for large columns
- Position lists consume memory (4 bytes per position)
- Very high selectivity (>80%) makes late materialization overhead > benefit
- Complex queries with many columns referenced everywhere reduce benefit

## Relational Algebra

```
-- Early materialization (row-oriented):
Scan(R) -> Filter(pred) -> Project(cols) -> Output
  -- Scan produces full rows, all columns materialized

-- Late materialization (column-oriented):
positions = Filter(Scan(R, [pred_cols]), pred)
output = Gather(R, positions, [output_cols])
  -- Filter only reads predicate columns
  -- Gather only reads output columns at qualifying positions

-- Invisible join (Abadi):
positions_R = Filter(Scan(R, [pred_cols_R]), pred_R)
positions_S = JoinProbe(positions_R, S.fk_col, ht_S)
output = Gather(R, positions_S, [output_cols_R])
       + Gather(S, HT_positions(positions_S), [output_cols_S])
```

## Implementation

```rust
/// Materialization strategy for a query plan
pub enum MaterializationStrategy {
    /// Reconstruct rows immediately after scan
    Early,
    /// Pass position lists, materialize at output
    Late,
    /// Materialize specific column groups early,
    /// keep others late
    Hybrid {
        early_columns: Vec<ColumnId>,
        late_columns: Vec<ColumnId>,
    },
}

/// Position list: references into base column storage
pub struct PositionList {
    /// Table the positions refer to
    table_id: TableId,
    /// Qualifying row positions (sorted, no duplicates)
    positions: Vec<u32>,
    /// Original column length
    source_len: usize,
}

/// Late materialization query executor
pub struct LateMaterializationExecutor {
    strategy: MaterializationStrategy,
}

impl LateMaterializationExecutor {
    /// Execute query with late materialization
    pub fn execute(
        &self,
        plan: &QueryPlan,
        storage: &ColumnStorage,
    ) -> Vec<Row> {
        match plan {
            QueryPlan::ScanFilterProject {
                table, filter_cols, filter_pred,
                output_cols,
            } => {
                // Phase 1: Scan only filter columns
                let filter_data: Vec<ColumnArray> =
                    filter_cols.iter().map(|&col_id| {
                        storage.read_column(table, col_id)
                    }).collect();

                // Phase 2: Evaluate filter -> position list
                let positions = evaluate_filter(
                    &filter_data, filter_pred,
                );

                // Phase 3: Gather output columns at positions
                let output_data: Vec<ColumnArray> =
                    output_cols.iter().map(|&col_id| {
                        let full_col = storage.read_column(
                            table, col_id,
                        );
                        gather(&full_col, &positions)
                    }).collect();

                // Phase 4: Stitch into rows (final output)
                stitch_columns_to_rows(&output_data)
            }

            QueryPlan::JoinFilterProject {
                outer_table, inner_table,
                join_key_outer, join_key_inner,
                filter_pred, output_cols,
            } => {
                // Invisible join pattern
                self.invisible_join(
                    storage,
                    outer_table, inner_table,
                    *join_key_outer, *join_key_inner,
                    filter_pred, output_cols,
                )
            }

            _ => {
                // Fallback: early materialization
                self.execute_early(plan, storage)
            }
        }
    }

    /// Invisible join (Abadi et al.)
    fn invisible_join(
        &self,
        storage: &ColumnStorage,
        fact_table: &TableId,
        dim_table: &TableId,
        fact_fk: ColumnId,
        dim_pk: ColumnId,
        dim_filter: &Option<FilterPredicate>,
        output_cols: &[OutputColumn],
    ) -> Vec<Row> {
        // Step 1: Filter dimension table
        let dim_positions = match dim_filter {
            Some(pred) => {
                let filter_cols = pred.referenced_columns();
                let cols: Vec<ColumnArray> = filter_cols
                    .iter()
                    .map(|&c| {
                        storage.read_column(dim_table, c)
                    })
                    .collect();
                evaluate_filter(&cols, pred)
            }
            None => {
                let n = storage.row_count(dim_table);
                SelectionVector {
                    source_len: n,
                    positions: (0..n as u32).collect(),
                }
            }
        };

        // Step 2: Build hash set of qualifying dim PKs
        let dim_pk_col = storage.read_column(
            dim_table, dim_pk,
        );
        let qualifying_pks = gather(
            &dim_pk_col, &dim_positions,
        );
        let pk_set = build_hash_set(&qualifying_pks);

        // Step 3: Scan fact FK column, probe hash set
        //   Produces position list into fact table
        let fact_fk_col = storage.read_column(
            fact_table, fact_fk,
        );
        let fact_positions = probe_hash_set(
            &fact_fk_col, &pk_set,
        );

        // Step 4: Late gather: only now read payload
        let mut output_columns = Vec::new();
        for oc in output_cols {
            let col = storage.read_column(
                &oc.table_id, oc.column_id,
            );
            let sel = if oc.table_id == *fact_table {
                &fact_positions
            } else {
                &dim_positions
            };
            output_columns.push(gather(&col, sel));
        }

        stitch_columns_to_rows(&output_columns)
    }
}

/// Decide materialization strategy based on query
pub fn choose_materialization_strategy(
    plan: &QueryPlan,
    stats: &TableStatistics,
) -> MaterializationStrategy {
    let selectivity = estimate_selectivity(plan, stats);
    let num_output_cols = plan.output_columns().len();
    let num_filter_cols = plan.filter_columns().len();
    let avg_col_width = stats.avg_column_width;

    // Benefit of late materialization:
    //   avoids reading (num_output_cols - num_filter_cols)
    //   columns for (1 - selectivity) fraction of rows
    let bytes_saved_late = stats.row_count as f64
        * (1.0 - selectivity)
        * (num_output_cols - num_filter_cols) as f64
        * avg_col_width as f64;

    // Cost of late materialization:
    //   random gather on output columns for qualifying rows
    let gather_cost = stats.row_count as f64
        * selectivity
        * num_output_cols as f64
        * 50.0; // ~50 ns per random gather (cache miss)

    let scan_cost = stats.row_count as f64
        * num_output_cols as f64
        * 1.0; // ~1 ns per sequential value

    if bytes_saved_late > gather_cost as f64 {
        MaterializationStrategy::Late
    } else {
        MaterializationStrategy::Early
    }
}

/// Stitch column arrays back into rows at output
fn stitch_columns_to_rows(
    columns: &[ColumnArray],
) -> Vec<Row> {
    let num_rows = columns[0].len;
    let mut rows = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let mut row = Row::new();
        for col in columns {
            row.push(col.value_at(i));
        }
        rows.push(row);
    }

    rows
}
```

## Cost Model

**Late vs. Early Materialization Break-Even:**

| Selectivity | Output Cols | Late Benefit | Gather Cost | Winner |
|------------|------------|-------------|-------------|--------|
| 1% | 10 | 99% cols saved | 1% x 50ns gather | Late |
| 10% | 10 | 90% cols saved | 10% x 50ns gather | Late |
| 50% | 10 | 50% cols saved | 50% x 50ns gather | Depends |
| 90% | 10 | 10% cols saved | 90% x 50ns gather | Early |
| 1% | 2 | 99% x 1 col saved | 1% x 50ns gather | Late |
| 90% | 2 | 10% x 1 col saved | 90% x 50ns gather | Early |

**General rule:** Late materialization wins when `selectivity < 30%` or `num_payload_cols >> num_filter_cols`.

**Gather Cost:**
- Sequential positions: ~4 ns/value (prefetchable)
- Random positions within L2: ~10 ns/value
- Random positions in L3: ~30-50 ns/value
- Random positions in DRAM: ~80-100 ns/value

**Memory:**
- Position list: 4 bytes per qualifying row
- Late: position lists + filter columns only in memory
- Early: full rows in memory for all qualifying tuples
- Savings: `qualifying_rows x (row_width - position_width)`

## Test Cases

```sql
-- Test 1: High selectivity filter (late materialization wins)
SELECT * FROM events WHERE type = 'CRITICAL';
-- 0.1% of rows qualify: read type column, gather 5 payload cols
-- Late: scan 1 col + gather 5 cols at 0.1% positions
-- Early: scan all 6 cols, discard 99.9%
-- Late saves: 5 columns x 99.9% rows = massive I/O savings

-- Test 2: Star schema join (invisible join)
SELECT p.name, SUM(s.quantity)
FROM sales s JOIN products p ON s.product_id = p.id
WHERE p.category = 'Electronics';
-- Filter products first: small dimension table
-- Probe sales.product_id against qualifying product IDs
-- Only then gather sales.quantity at matching positions
-- Never read product.name until final output

-- Test 3: Low selectivity (early materialization wins)
SELECT * FROM logs WHERE severity >= 'INFO';
-- 95% of rows qualify: gather nearly all positions
-- Late: position list + random gather = worse than sequential
-- Early: sequential scan of all columns = better

-- Test 4: Many-column projection after narrow filter
SELECT col1, col2, ..., col20
FROM wide_table WHERE narrow_filter_col = 42;
-- 0.5% selectivity, 20 output columns
-- Late: scan 1 filter col + gather 20 cols at 0.5% positions
-- Early: scan all 21 columns
-- Late saves: 20 columns x 99.5% rows
```

## Comparison

| Property | Late Materialization | Early Materialization | Hybrid |
|----------|--------------------|--------------------|--------|
| Filter scan | Predicate cols only | All columns | Grouped |
| I/O for low sel | Minimal (pred cols) | Full scan | Medium |
| Gather cost | Random per col | None | Partial |
| Memory pressure | Low (positions) | High (full rows) | Medium |
| Break-even sel | <30% typically | >30% | Varies |
| Implementation | Complex | Simple | Complex |
| Best for | OLAP, star schema | OLTP, wide output | Mixed |

## References

1. **Abadi, Daniel J.; Myers, Daniel S.; DeWitt, David J.; Madden, Samuel R.**. "Materialization Strategies in a Column-Oriented DBMS." ICDE 2007.
2. **Abadi, Daniel J.; Madden, Samuel R.; Hachem, Nabil**. "Column-Stores vs. Row-Stores: How Different Are They Really?" SIGMOD 2008.
3. **Boncz, Peter A.; Zukowski, Marcin; Nes, Niels**. "MonetDB/X100: Hyper-Pipelining Query Execution." CIDR 2005.
4. **Idreos, Stratos; Groffen, Fabian; Nes, Niels; Manegold, Stefan; Mullender, Sjoerd; Boncz, Peter**. "MonetDB: Two Decades of Research in Column-oriented Database Architectures." IEEE Data Eng. Bull. 2012.
