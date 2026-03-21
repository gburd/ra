# Rule: Oracle Batched Table Access by ROWID

**Category:** database-specific/oracle
**File:** `rules/database-specific/oracle/batch-table-access.rra`

## Metadata

- **ID:** `oracle-batch-table-access`
- **Version:** "1.0.0"
- **Databases:** oracle
- **Tags:** database-specific, oracle, batch, rowid, index, table-access
- **Authors:** "RA Contributors"


# Oracle Batched Table Access by ROWID

## Description

Batches table accesses by ROWID obtained from an index scan, sorting
the ROWIDs by physical block address before fetching table rows.
This converts random I/O into sequential I/O, reducing physical reads
on spinning disks and improving buffer cache efficiency.

**When to apply**: An index range scan returns ROWIDs that are then
used to access the table (TABLE ACCESS BY INDEX ROWID).

**Why it works**: Index-ordered ROWIDs point to scattered table blocks
(especially for secondary indexes).  Sorting ROWIDs by block address
groups accesses to the same block, enabling multi-block I/O and
reducing the number of physical reads from spinning disks by up to
50%.

**Database version**: Oracle 12cR1+

## Relational Algebra

```algebra
-- Before: random ROWID access
table-access-by-rowid(index-range-scan(T, idx, pred))

-- After: batched sorted ROWID access
table-access-by-rowid-batched(
    sort-by-block(index-range-scan(T, idx, pred)))
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("oracle-batch-table-access-by-rowid";
    "(table-access-by-rowid
        (index-range-scan ?table ?index ?pred))" =>
    "(table-access-by-rowid-batched
        (sort-rowids-by-block
            (index-range-scan ?table ?index ?pred)))"
    if is_database("oracle")
    if rowid_count_exceeds_threshold("?pred", "?table")
),
```

## Preconditions

```rust
fn applicable(
    index_scan_rows: f64,
    table_blocks: f64,
) -> bool {
    // Beneficial when ROWIDs are scattered across many blocks
    let clustering_factor_ratio =
        index_scan_rows / table_blocks;
    clustering_factor_ratio > 0.1  // poorly clustered
}
```

**Restrictions:**
- Only beneficial for secondary indexes (primary/IOT already clustered)
- Negligible benefit on SSD storage (random I/O is fast)
- Adds memory overhead for ROWID sort buffer
- _OPTIMIZER_BATCH_TABLE_ACCESS_BY_ROWID controls this feature

## Cost Model

```rust
fn estimated_benefit(
    rowid_count: f64,
    table_blocks: f64,
    is_ssd: bool,
) -> f64 {
    if is_ssd {
        return 0.0; // Minimal benefit on flash storage
    }
    // Random reads avoided by batching
    let random_reads = rowid_count * 0.8; // not all are unique blocks
    let sequential_reads = table_blocks * 0.3; // after sorting
    (random_reads - sequential_reads) * 10.0 // ms per random read
}
```

**Typical benefit**: For 100K ROWIDs on spinning disk, reduces
I/O from ~100K random reads to ~30K sequential reads.

## Test Cases

```sql
-- Positive: secondary index scan with scattered ROWIDs
SELECT * FROM orders WHERE status = 'pending';
-- status index returns scattered ROWIDs; batched access helps
```

```sql
-- Negative: primary key lookup (single ROWID)
SELECT * FROM orders WHERE order_id = 12345;
-- Single ROWID; no batching needed
```

## References

Oracle: Oracle Database SQL Tuning Guide, "Batched Table Access"
Oracle: EXPLAIN PLAN TABLE ACCESS BY INDEX ROWID BATCHED
