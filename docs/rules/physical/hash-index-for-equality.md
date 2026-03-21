# Rule: Hash Index for Equality Predicates

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/hash-index-for-equality.rra`

## Metadata

- **ID:** `hash-index-for-equality`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql
- **Tags:** index, hash, equality, point-lookup
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (= ?col ?val) (scan ?table))"
    description: "Equality predicate on table with hash index"
  - type: "predicate"
    condition: "has_hash_index(?table, ?col)"
    description: "Hash index must exist on the equality column"
  - type: "predicate"
    condition: "is_equality_only_predicate(?col, ?val)"
    description: "Predicate must be pure equality (no range, no ORDER BY)"
```


# Hash Index for Equality Predicates

## Description

Uses a hash index for equality-only predicates. Hash indexes provide
O(1) lookup time for exact-match queries, which is faster than
B-tree's O(log n) when no range scans or ordering is needed.

**When to apply**: A query has equality predicates only (no range,
ORDER BY, or inequality) and a hash index exists on the column.

**Why it works**: Hash indexes compute a hash of the key and directly
locate the bucket containing matching rows. This avoids tree
traversal, reducing lookup cost from O(log n) to O(1).

## Relational Algebra

```algebra
filter[col = value](scan[T])
  -> hash_index_scan[I](col, value)
  where I is a hash index on column col
```

## Implementation

```rust
rw!("hash-index-for-equality";
    "(filter (= ?col ?val) (scan ?table))" =>
    "(hash-index-scan ?index ?col ?val)"
    if has_hash_index_on("?table", "?col")
    if is_equality_only_predicate("?col", "?val")
),
```

## Cost Model

```rust
fn cost(matching_rows: u64) -> f64 {
    let hash_compute = 1.0;
    let bucket_access = 1.0;
    let overflow_chain = matching_rows.min(3) as f64;
    let tuple_fetch = matching_rows as f64 * 1.5;
    hash_compute + bucket_access + overflow_chain + tuple_fetch
}
```

**Typical benefit**: 30-80% over B-tree for pure equality lookups.

## Test Cases

### Positive: Equality lookup with hash index

```sql
CREATE INDEX idx_sessions_token ON sessions USING HASH(session_token);

SELECT * FROM sessions WHERE session_token = 'abc123def456';

-- O(1) hash lookup vs O(log n) B-tree traversal
```

### Negative: Range predicate

```sql
SELECT * FROM sessions WHERE session_token > 'abc';

-- Hash indexes do not support range scans
```

### Negative: ORDER BY on hash-indexed column

```sql
SELECT * FROM sessions ORDER BY session_token;

-- Hash indexes do not maintain order
```

## References

- PostgreSQL: Hash indexes (WAL-logged since v10)
- IndexType::Hash in ra-stats/src/index_types.rs
- IndexCostFactors::hash_default() (infinite range_scan_cost)
