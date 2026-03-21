# Rule: Remove Casts That Prevent Index Usage

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/cast-elimination-for-index.rra`

## Metadata

- **ID:** `cast-elimination-for-index`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql
- **Tags:** logical, function, index, cast, type-coercion, sargable
- **Authors:** "RA Contributors"


# Remove Casts That Prevent Index Usage

## Description

Eliminates unnecessary type casts on indexed columns that would
prevent index use. When CAST(int_col AS BIGINT) = 42 wraps an
indexed INT column, the cast can be removed because INT values
are a subset of BIGINT. Similarly, CAST(varchar_col AS TEXT)
is a no-op in many databases. The optimizer moves the cast to
the comparison value side when safe.

**When to apply**: A CAST on an indexed column can be eliminated
because the cast is lossless (widening) or the comparison value
can be cast to the column's native type instead.

**Why it works**: Any function applied to an indexed column
prevents the optimizer from using the index (not SARGable). By
removing the cast from the column and optionally casting the
comparison value, the bare column enables index access.

## Implementation

```rust
// Remove widening cast (int -> bigint, varchar -> text)
rw!("remove-widening-cast-for-index";
    "(filter (= (cast ?col ?wide_type) ?val) (scan ?t))" =>
    "(filter (= ?col (cast ?val ?narrow_type))
       (index-scan ?t ?idx))"
    if has_index("?t", "?col", "?idx")
    if is_widening_cast("?col", "?wide_type")
    if get_column_type("?col", "?narrow_type")
),

// Remove identity cast (same type)
rw!("remove-identity-cast";
    "(cast ?col ?type)" => "?col"
    if column_has_type("?col", "?type")
),

// Move cast from column to value for numeric types
rw!("move-numeric-cast-to-value";
    "(filter (= (cast ?col 'numeric') ?val) (scan ?t))" =>
    "(filter (= ?col (cast ?val ?col_type))
       (index-scan ?t ?idx))"
    if has_index("?t", "?col", "?idx")
    if is_exact_numeric_type("?col", "?col_type")
    if value_fits_in_type("?val", "?col_type")
),

// Remove varchar length extension cast
rw!("remove-varchar-extension-cast";
    "(filter (= (cast ?col 'varchar(?n)') ?val) (scan ?t))" =>
    "(filter (= ?col ?val) (index-scan ?t ?idx))"
    if has_index("?t", "?col", "?idx")
    if column_type_fits("?col", "varchar(?n)")
),
```

## Preconditions

- Cast must be lossless (widening): no precision loss, no truncation
- Widening casts: INT->BIGINT, FLOAT->DOUBLE, VARCHAR(n)->VARCHAR(m)
  where m >= n, VARCHAR->TEXT, DATE->TIMESTAMP
- Comparison value must be representable in the column's native type
- Narrowing casts (BIGINT->INT) cannot be eliminated without range check

## Test Cases

```sql
-- Setup: CREATE INDEX idx_id ON orders (id);  -- id is INT

-- Positive: widening cast INT to BIGINT
SELECT * FROM orders WHERE CAST(id AS BIGINT) = 42;
-- Rewritten: WHERE id = 42 (using index)

-- Positive: identity cast
SELECT * FROM orders WHERE CAST(id AS INTEGER) = 42;
-- Cast removed: WHERE id = 42

-- Setup: CREATE INDEX idx_name ON users (name);  -- name is VARCHAR(50)

-- Positive: varchar to text cast
SELECT * FROM users WHERE CAST(name AS TEXT) = 'Alice';
-- Rewritten: WHERE name = 'Alice' (using index)

-- Positive: varchar length extension
SELECT * FROM users WHERE CAST(name AS VARCHAR(100)) = 'Alice';
-- Rewritten: WHERE name = 'Alice' (50 fits in 100)

-- Negative: narrowing cast (potential data loss)
SELECT * FROM orders WHERE CAST(id AS SMALLINT) = 42;
-- Cannot remove: SMALLINT is narrower than INT

-- Negative: value doesn't fit in column type
SELECT * FROM orders WHERE CAST(id AS BIGINT) = 3000000000;
-- Cannot move cast: 3 billion exceeds INT range
```

## References

- PostgreSQL: Type Conversion and Index Usage
- mssql: Implicit Conversion and SARGability
- "SARGable Predicates" in query optimization literature
