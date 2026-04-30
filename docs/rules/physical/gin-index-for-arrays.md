# Rule: GIN Index for Array Operations

**Category:** physical/index-selection
**File:** `rules/physical/index-selection/gin-index-for-arrays.rra`

## Metadata

- **ID:** `gin-index-for-arrays`
- **Version:** "1.0.0"
- **Databases:** postgresql
- **Tags:** index, gin, array, jsonb, postgresql, containment
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter (contains ?col ?val) (scan ?table))"
    description: "Array containment filter with GIN index"
  - type: "predicate"
    condition: "has_gin_index(?table, ?col)"
    description: "GIN index must exist on the array column"
  - type: "capability"
    database: "current"
    requires: "gin_index"
    description: "Database supports GIN indexes (PostgreSQL)"
```


# GIN Index for Array Operations

## Description

Uses a PostgreSQL GIN (Generalized Inverted Index) for array
containment, overlap, and JSONB operations. GIN indexes decompose
composite values into individual elements and build an inverted index,
enabling efficient searches for "contains element" queries.

**When to apply**: A query uses array operators (@>, &&, <@) or
JSONB containment (@>) on a column with a GIN index.

**Why it works**: GIN indexes break arrays/JSONB into individual
keys and map each key to the set of rows containing it. Checking
containment becomes an intersection of posting lists rather than
scanning every row's array.

## Relational Algebra

```algebra
filter[tags @> ARRAY['rust']](scan[T])
  -> gin_index_scan[I](tags, @>, ARRAY['rust'])
  where I is a GIN index on column tags
```

## Implementation

```rust
rw!("gin-index-for-array-contains";
    "(filter (array-contains ?col ?elements) (scan ?table))" =>
    "(gin-index-scan ?index ?col array-contains ?elements)"
    if has_gin_index_on("?table", "?col")
),

rw!("gin-index-for-jsonb-contains";
    "(filter (jsonb-contains ?col ?pattern) (scan ?table))" =>
    "(gin-index-scan ?index ?col jsonb-contains ?pattern)"
    if has_gin_index_on("?table", "?col")
),
```

## Cost Model

```rust
fn cost(
    posting_list_entries: u64,
    search_keys: usize,
    matching_rows: u64,
) -> f64 {
    let index_lookups = search_keys as f64 * 3.0; // Per-key GIN lookup
    let posting_scan = posting_list_entries as f64 * 0.5;
    let tuple_fetch = matching_rows as f64 * 2.0;
    index_lookups + posting_scan + tuple_fetch
}
```

**Typical benefit**: 50-95% for selective containment queries.

## Test Cases

### Positive: Array containment

```sql
CREATE INDEX idx_articles_tags ON articles USING GIN(tags);

SELECT * FROM articles WHERE tags @> ARRAY['database', 'optimization'];

-- GIN intersects posting lists for 'database' and 'optimization'
```

### Positive: JSONB containment

```sql
CREATE INDEX idx_events_data ON events USING GIN(metadata jsonb_ops);

SELECT * FROM events WHERE metadata @> '{"type": "purchase"}';

-- GIN lookup for the key-value pair
```

### Negative: Equality on scalar (B-tree better)

```sql
SELECT * FROM users WHERE id = 42;

-- Scalar equality: B-tree index is more efficient
```

## References

- PostgreSQL: GIN indexes for arrays, JSONB, full-text
- IndexType::GIN in ra-stats-advanced/src/index_types.rs
- IndexCostFactors::gin_default()
