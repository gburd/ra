# Rule: Row-to-Columnar Conversion for Device Processing

**Category:** hardware/data-placement
**File:** `rules/hardware/data-placement/columnar-conversion.rra`

## Metadata

- **ID:** `columnar-conversion`
- **Version:** "1.0.0"
- **Databases:** heavydb, pg-strom, blazingsql
- **Tags:** columnar, row-store, conversion, layout, gpu, transpose
- **Authors:** "RA Contributors"


# Row-to-Columnar Conversion for Device Processing

## Description

Converts row-oriented data to columnar layout before transferring to
a GPU or FPGA. Accelerators process columnar data more efficiently:
GPU SIMT threads access contiguous memory for the same column, and
FPGA pipelines consume one column stream at a time. The conversion
cost is amortized over the speedup from better device utilization.

**When to apply**: The source data is in row format (from a row-store
database or network transfer) and the query will be executed on a
GPU or FPGA. The number of columns actually needed by the query
determines whether conversion is worthwhile.

**Why it works**: Row stores interleave columns, causing poor memory
coalescing on GPUs (threads in a warp access non-contiguous memory).
Columnar layout ensures that all threads in a warp access consecutive
addresses, achieving full memory bandwidth. The transpose cost is
O(n*c) but runs on CPU cache-efficiently using blocked transpose.

## Relational Algebra

```algebra
gpu_op(row_data(R)) -> gpu_op(to_columnar(row_data(R)))
  where query uses only k << total_columns columns
    AND gpu_coalescing_benefit > transpose_cost
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("row-to-columnar-for-gpu";
    "(gpu_scan (row_format ?input))" =>
    "(gpu_scan (to_columnar ?input))"
    if few_columns_needed("?input")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    needed_columns: usize,
    total_columns: usize,
    hw: &HardwareProfile,
) -> bool {
    // Columnar conversion is worthwhile when:
    // 1. Few columns are needed (projection pushdown)
    // 2. Table is large enough to amortize conversion
    let selectivity =
        needed_columns as f64 / total_columns as f64;

    stats.row_count > 10_000.0
        && selectivity < 0.5
        && hw.gpu_available
}
```

**Restrictions:**
- Conversion has CPU cost that must be amortized
- Variable-length fields require offset arrays
- Null bitmasks must be generated per column
- Already-columnar data does not need conversion

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    needed_cols: usize,
    total_cols: usize,
    hw: &HardwareProfile,
) -> f64 {
    let n = stats.row_count;
    let row_bytes = stats.avg_row_size;

    // Transpose cost: ~2ns per element (cache-efficient blocked)
    let transpose_ns =
        n * total_cols as f64 * 2.0;

    // Transfer savings: only transfer needed columns
    let row_transfer_bytes = n as u64 * row_bytes;
    let col_transfer_bytes =
        n as u64 * (row_bytes / total_cols as u64)
            * needed_cols as u64;
    let transfer_saved_ns =
        (row_transfer_bytes - col_transfer_bytes) as f64
            / (hw.pcie_bandwidth_gbps * 1e9)
            * 1e9;

    // GPU coalescing benefit: ~2-4x for columnar vs row
    let gpu_speedup_ns =
        n * 10.0 * (1.0 - 1.0 / 3.0); // ~2/3 of GPU time saved

    let benefit = transfer_saved_ns + gpu_speedup_ns;
    if benefit > transpose_ns {
        (benefit - transpose_ns) / benefit
    } else {
        0.0
    }
}
```

**Typical benefit**: 2x-4x GPU throughput improvement from coalesced
memory access, minus the one-time transpose cost.

## Test Cases

### Positive: Wide table, few columns needed

```sql
-- web_events: 50 columns, 100M rows, row-oriented source
-- Query needs only 3 columns
SELECT event_type, user_id, COUNT(*)
FROM web_events
WHERE event_type = 'purchase'
GROUP BY event_type, user_id;

-- Expected: Convert to columnar, transfer 3 columns to GPU
-- Plan: GpuAggregate(to_columnar(
--        Project([event_type, user_id], web_events)))
```

### Negative: Already columnar storage

```sql
-- DuckDB native columnar format, no conversion needed
SELECT l_returnflag, SUM(l_quantity)
FROM lineitem
GROUP BY l_returnflag;

-- Plan: GpuAggregate(lineitem) -- already columnar
```

## References

**Implementation in databases:**
- PG-Strom: Row-to-column conversion for GPU processing
- HeavyDB: Columnar storage native to GPU
- Apache Arrow: Columnar format as interchange

**Academic papers:**
- Pirk et al., "Waste Not... Efficient Co-Processing of Relational Data", ICDE 2014
- Heimel et al., "Hardware-Oblivious Parallelism for In-Memory Column-Stores", VLDB 2013
