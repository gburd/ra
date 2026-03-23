# Rule: Generate Secondary Index Scans

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/generate-index-scans.rra`

## Metadata

- **ID:** `cockroachdb-generate-index-scans`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, index, scan, exploration
- **Authors:** "RA Contributors"


# Generate Secondary Index Scans

## Description

Creates alternate Scan expressions for each secondary index on the scanned table during exploration. This allows the optimizer to evaluate different index access paths and choose the most efficient one based on query predicates and required columns.

**When to apply**: During exploration phase for any canonical table scan with secondary indexes.

**Why it works**: Different indexes may be more efficient for different query patterns. Secondary indexes can provide better selectivity, avoid sorting, or enable index-only scans. Generating all alternatives allows cost-based selection of the best access path.

**Database version**: CockroachDB v2.0+

## Relational Algebra

```algebra
Scan[primary_index](T)
  -> {Scan[idx](T) | idx $\in$ indexes(T)}
  where is_canonical_scan
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-generate-index-scans";
    "(scan ?private)" =>
    "(generate_index_scans ?private)"
    if is_database("cockroachdb")
    if is_canonical_scan("?private")
),
```

## Preconditions

```rust
fn applicable(scan: &ScanPrivate) -> bool {
    // Must be a canonical scan (not already using a specific index hint)
    is_canonical_scan(scan)
        // Table must have secondary indexes
        && scan.table().has_secondary_indexes()
}
```

**Restrictions:**
- Only applies to CockroachDB
- Applies during exploration phase
- Table must have secondary indexes
- Does not re-generate for scans that already specify an index

## Cost Model

```rust
fn select_best_index(
    query_predicates: &[Expr],
    required_cols: &[Column],
    indexes: &[Index],
    stats: &Statistics,
) -> &Index {
    let mut best_index = &indexes[0]; // primary
    let mut best_cost = f64::MAX;

    for idx in indexes {
        let cost = estimate_index_cost(
            idx,
            query_predicates,
            required_cols,
            stats,
        );
        if cost < best_cost {
            best_cost = cost;
            best_index = idx;
        }
    }

    best_index
}
```

**Typical benefit**: 30-90% when better index available for query

## Test Cases

### Positive Case 1: Index on Filter Column

```sql
CREATE TABLE users (
  id INT PRIMARY KEY,
  email STRING,
  created_at TIMESTAMP,
  INDEX email_idx (email),
  INDEX created_idx (created_at)
);

SELECT * FROM users WHERE email = 'user@example.com';

-- Generates scans:
-- 1. Scan users@primary
-- 2. Scan users@email_idx (likely chosen - selective on email)
-- 3. Scan users@created_idx
```

### Positive Case 2: Index-Only Scan

```sql
SELECT email, created_at FROM users
WHERE created_at > '2024-01-01';

-- created_idx may allow index-only scan (no need to fetch from primary)
```

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/scan.opt`
  - Rule: `GenerateIndexScans` (lines 5-10)
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
