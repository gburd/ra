# Rule: GPU Accelerated String Operations

**Category:** hardware/gpu
**File:** `rules/hardware/gpu/gpu-string-operations.rra`

## Metadata

- **ID:** `gpu-string-operations`
- **Version:** "1.0.0"
- **Databases:** heavydb, sqream, blazingsql
- **Tags:** gpu, string, like, regex, parallel
- **Authors:** "RA Contributors"


# GPU Accelerated String Operations

## Description

Offloads string-heavy operations (LIKE, REGEXP, SUBSTR, CONCAT,
string comparison) to the GPU. Each GPU thread processes one string,
evaluating patterns or transformations in parallel. This is effective
for bulk string filtering and transformation on large text-heavy
datasets.

**When to apply**: A query performs string matching or transformation
over a large column. The strings should be stored in a
dictionary-encoded or length-prefixed columnar format for efficient
GPU transfer.

**Why it works**: String operations are inherently per-row independent.
The GPU can process thousands of strings simultaneously. For pattern
matching (LIKE/REGEX), the GPU compiles the pattern into a DFA/NFA
that each thread evaluates against its assigned string. Dictionary
encoding enables the GPU to match against the dictionary and broadcast
results.

## Relational Algebra

```algebra
sigma[LIKE(col, pattern)](R) -> gpu_string_filter[LIKE(col, pattern)](R)
  where |R| > threshold
    AND col is string type
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("gpu-string-filter";
    "(filter (like ?col ?pattern) ?input)" =>
    "(gpu_string_filter (like ?col ?pattern) ?input)"
    if input_large_enough("?input")
    if column_is_string("?col")
),

rw!("gpu-string-regex";
    "(filter (regexp ?col ?pattern) ?input)" =>
    "(gpu_string_filter (regexp ?col ?pattern) ?input)"
    if input_large_enough("?input")
    if column_is_string("?col")
),
```

## Preconditions

```rust
fn applicable(
    stats: &Statistics,
    col_name: &str,
    hw: &HardwareProfile,
) -> bool {
    let col_stats = stats.columns.get(col_name);
    let avg_len = col_stats
        .and_then(|c| c.avg_length)
        .unwrap_or(32.0);

    let string_bytes =
        stats.row_count as u64 * avg_len as u64;

    stats.row_count > 100_000.0
        && (string_bytes <= hw.gpu_memory_bytes
            || hw.chunked_transfer_enabled)
}
```

**Restrictions:**
- Very long strings (>4KB) cause warp divergence and memory pressure
- Backreference-heavy regex patterns cannot be parallelized as DFAs
- Dictionary-encoded strings get much better GPU performance

## Cost Model

```rust
fn estimated_benefit(
    stats: &Statistics,
    avg_string_len: f64,
    hw: &HardwareProfile,
) -> f64 {
    let n = stats.row_count;
    let string_bytes = n as u64 * avg_string_len as u64;

    // CPU: sequential string matching
    let cpu_ns = n * avg_string_len * 2.0; // ~2ns per char

    // GPU: transfer + parallel matching
    let transfer_ns = string_bytes as f64
        / (hw.pcie_bandwidth_gbps * 1e9) * 1e9;
    let gpu_match_ns =
        n * avg_string_len * 2.0 / hw.gpu_sm_count as f64;
    let gpu_ns = transfer_ns + gpu_match_ns;

    if cpu_ns > gpu_ns {
        (cpu_ns - gpu_ns) / cpu_ns
    } else {
        0.0
    }
}
```

**Typical benefit**: 5x-20x for bulk LIKE/REGEXP over millions of rows.
Dictionary-encoded columns can see 50x+ when matching is done on the
dictionary alone.

## Test Cases

### Positive: Bulk LIKE filtering

```sql
-- customer: 15M rows, avg c_comment length 73 chars
SELECT * FROM customer
WHERE c_comment LIKE '%unusual%accounts%';

-- Expected: GPU string filter
-- Plan: GpuStringFilter(like='%unusual%accounts%',
--        col=c_comment, input=customer)
```

### Positive: REGEXP on large table

```sql
SELECT * FROM web_logs
WHERE url REGEXP '^/api/v[0-9]+/users/[0-9]+'

-- Expected: GPU regex evaluation (DFA compiled for GPU)
-- Plan: GpuStringFilter(regexp=..., col=url, input=web_logs)
```

### Negative: Small table

```sql
SELECT * FROM categories WHERE name LIKE '%electronics%';

-- Expected: CPU string matching
-- Plan: Filter(like='%electronics%', col=name, input=categories)
```

## References

**Implementation in databases:**
- HeavyDB: GPU string dictionary processing
- SQream: GPU-native string operations
- NVIDIA cuDF: `cudf::strings` module

**Academic papers:**
- Zu et al., "GPU-based NFA Implementation for Memory Efficient High Speed Regular Expression Matching", PPoPP 2012
- Mytkowicz et al., "Data-Parallel Finite-State Machines", ASPLOS 2014
