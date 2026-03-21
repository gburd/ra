# Rule: Apache Derby Bulk Fetch Optimization

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/bulk-fetch.rra`

## Metadata

- **ID:** `derby-bulk-fetch`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, bulk-fetch, scan, prefetch, buffer
- **Authors:** "RA Contributors"


# Apache Derby Bulk Fetch Optimization

## Description

Derby reads multiple rows at once during table and index scans using
bulk fetch.  Instead of one row per storage engine call, Derby fetches
a batch of rows (default 16) into a buffer, reducing per-row overhead
from storage engine interactions.

**When to apply**: A full table scan or index range scan reads many
consecutive rows.

**Why it works**: Each interaction with the storage engine has fixed
overhead (function calls, lock checks).  Fetching 16 rows per call
amortizes this overhead, and sequential reads benefit from OS
read-ahead.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: row-at-a-time scan
scan(table, fetch_size=1)

-- After: bulk fetch scan
scan(table, fetch_size=16)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-bulk-fetch";
    "(scan ?table)" =>
    "(bulk-scan ?table 16)"
    if is_database("derby")
    if is_sequential_scan("?table")
),
```

## Preconditions

```rust
fn applicable(scan: &Scan) -> bool {
    scan.is_full_table_scan() || scan.is_range_scan()
}
```

**Restrictions:**
- Bulk fetch size is configurable via Derby properties
- May increase memory usage proportional to batch size * row width
- Not applicable to single-row lookups (point queries)

## Cost Model

```rust
fn estimated_benefit(
    rows: f64,
    per_call_overhead: f64,
    fetch_size: f64,
) -> f64 {
    let single_fetch_calls = rows;
    let bulk_fetch_calls = rows / fetch_size;
    (single_fetch_calls - bulk_fetch_calls) * per_call_overhead
}
```

**Typical benefit**: 10-30% improvement for large sequential scans.

## Test Cases

```sql
-- Positive: full table scan
SELECT * FROM large_table;
-- Bulk fetch reads 16 rows per engine call
```

```sql
-- Negative: point lookup
SELECT * FROM users WHERE id = 42;
-- Single row; no bulk fetch benefit
```

## References

Apache Derby: Tuning Guide, "Bulk Fetch"
Source: org.apache.derby.impl.store.access.heap.HeapScan
