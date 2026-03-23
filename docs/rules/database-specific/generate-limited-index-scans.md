# Rule: Generate Limited Index Scans

**Category:** database-specific/cockroachdb
**File:** `rules/database-specific/cockroachdb/generate-limited-index-scans.rra`

## Metadata

- **ID:** `cockroachdb-generate-limited-index-scans`
- **Version:** 1.0.0
- **Databases:** cockroachdb
- **Tags:** database-specific, cockroachdb, limit, index, scan
- **Authors:** "RA Contributors"


# Generate Limited Index Scans

## Description

Generates a set of limited Scan operators (one per index) when a LIMIT is present. Each scan includes the limit, and an IndexJoin is added if the index doesn't provide all output columns. Pushing limits into scans is crucial for performance.

**When to apply**: LIMIT over a canonical table scan.

**Why it works**: Different indexes may provide different orderings and coverages. A non-covering index with a favorable ordering may still win with a limit pushdown.

**Database version**: CockroachDB v19.2+

## Relational Algebra

```algebra
Limit[k, order](Scan(T))
  -> {LimitedScan[idx, k, order](T) | idx $\in$ indexes(T)}
  where k > 0
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("cockroachdb-generate-limited-index-scans";
    "(limit (scan ?private) ?limit ?ordering)" =>
    "(generate_limited_scans ?private ?limit ?ordering)"
    if is_database("cockroachdb")
    if is_canonical_scan("?private")
    if is_positive_int("?limit")
),
```

## Preconditions

```rust
fn applicable(
    scan: &ScanPrivate,
    limit: u64,
) -> bool {
    is_canonical_scan(scan)
        && limit > 0
}
```

**Restrictions:**
- Only applies to CockroachDB
- Limit must be positive integer constant
- Non-covering indexes require IndexJoin

## Cost Model

```rust
fn estimated_benefit(
    table_rows: f64,
    limit: f64,
) -> f64 {
    let full_scan_cost = table_rows;
    let limited_scan_cost = limit.min(table_rows);
    (full_scan_cost - limited_scan_cost) / full_scan_cost
}
```

**Typical benefit**: 50-90% with small limits

## Test Cases

```sql
SELECT * FROM orders ORDER BY created_at DESC LIMIT 10;

-- Generates limited scans on:
-- 1. Primary index
-- 2. created_at index (if exists) - may be chosen
-- Each scan stops after finding 10 rows in the right order
```

## References

**Source code:**
- CockroachDB: `pkg/sql/opt/xform/rules/limit.opt`
  - Rule: `GenerateLimitedScans` (lines 5-18)
  - Commit: 6e210ba6aa33cea5e27b1a8fae212c27941781f4
