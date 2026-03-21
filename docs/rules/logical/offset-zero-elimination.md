# Rule: OFFSET Zero Elimination

**Category:** logical/limit-pushdown
**File:** `rules/logical/limit-pushdown/offset-zero-elimination.rra`

## Metadata

- **ID:** `offset-zero-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb
- **Tags:** offset, limit, simplification
- **Authors:** "RA Contributors"


# OFFSET Zero Elimination

## Description

Removes OFFSET 0 clauses which have no effect but add operator overhead.

**When to apply**: OFFSET with value 0.

**Why it works**: OFFSET 0 is identity; removing it simplifies plan.

## Relational Algebra

```algebra
offset[0](R) -> R
limit[K](offset[0](R)) -> limit[K](R)
```

## Implementation

```rust
rw!("offset-zero-elimination";
    "(offset 0 ?input)" => "?input"
),

rw!("limit-offset-zero";
    "(limit ?k (offset 0 ?input))" =>
    "(limit ?k ?input)"
),
```

## Cost Model

```rust
fn benefit() -> f64 {
    0.05 // Minor: eliminates no-op operator
}
```

**Typical benefit**: 0-10% (simplification only)

## Test Cases

### Positive: Explicit OFFSET 0

```sql
SELECT * FROM users LIMIT 10 OFFSET 0;

-- Remove OFFSET 0
```

### Positive: Parameterized query with offset=0

```sql
SELECT * FROM products LIMIT ? OFFSET ?;
-- When offset parameter is 0
```

### Negative: Non-zero offset

```sql
SELECT * FROM logs LIMIT 100 OFFSET 50;

-- Keep OFFSET: has semantic effect
```

## References

- All major databases: Constant folding eliminates OFFSET 0
- PostgreSQL: preprocess_limit for constant optimization
