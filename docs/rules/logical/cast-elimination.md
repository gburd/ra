# Rule: Cast Elimination

**Category:** logical/function-optimization
**File:** `rules/logical/function-optimization/cast-elimination.rra`

## Metadata

- **ID:** `cast-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, oracle, mssql, duckdb, sqlite
- **Tags:** function, cast, type-coercion, elimination, simplification
- **Authors:** "RA Contributors"


# Cast Elimination

## Description

Removes redundant type casts that have no effect or cancel each other out.
Implicit casts added by the query parser can accumulate, adding per-row
overhead without changing semantics.

**When to apply**: Cast expressions where:
- Source and target types are the same
- Cast is a no-op widening (INT to BIGINT where value fits)
- Two casts cancel out (CAST(CAST(x AS TEXT) AS INT) when x is INT)
- Cast is on a literal that can be parsed directly as the target type

**Why it works**: Each cast requires per-row type conversion. Eliminating
unnecessary casts removes this overhead and may enable index usage that
the cast would prevent.

## Relational Algebra

```algebra
CAST(x AS T) -> x               where typeof(x) = T
CAST(CAST(x AS T1) AS T2) -> CAST(x AS T2)  where T1 is lossless intermediate
CAST(literal AS T) -> literal_of_type_T     (compile-time conversion)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Same-type cast elimination
rw!("cast-same-type";
    "(cast ?expr ?type)" => "?expr"
    if expr_type_equals("?expr", "?type")
),

// Double cast collapse
rw!("cast-double-collapse";
    "(cast (cast ?expr ?intermediate) ?target)" =>
    "(cast ?expr ?target)"
    if is_lossless_cast("?expr", "?intermediate")
),

// Cast of literal -> typed literal
rw!("cast-literal-fold";
    "(cast (literal ?val) ?type)" =>
    { FoldCastLiteral { val: "?val", target_type: "?type" } }
),
```

**Restrictions:**
- Lossy casts (FLOAT to INT) cannot be eliminated
- Casts affecting collation or time zone must be preserved
- Implicit vs explicit casts have different precedence rules

## Cost Model

```rust
fn estimated_benefit(cast_cost: f64, num_rows: u64) -> f64 {
    let savings = cast_cost * num_rows as f64;
    savings / (savings + num_rows as f64)
}
```

**Typical benefit**: 5-40% for expression-heavy queries

## Test Cases

### Positive: Same-type cast

```sql
SELECT CAST(id AS INTEGER) FROM users;
-- id is already INTEGER -> remove cast
-- Rewrite to: SELECT id FROM users;
```

### Positive: Double cast on literal

```sql
SELECT * FROM t WHERE col = CAST(CAST('42' AS TEXT) AS INTEGER);
-- Fold to: WHERE col = 42
```

### Positive: Implicit widening cast

```sql
-- Parser adds: CAST(small_int_col AS INTEGER) for comparison
SELECT * FROM t WHERE small_int_col = 42;
-- Remove implicit widening if comparison is type-safe
```

### Negative: Lossy cast (preserves semantics)

```sql
SELECT CAST(price AS INTEGER) FROM products;
-- FLOAT -> INTEGER is lossy (truncation), must keep
```

### Negative: Cast affecting index usage

```sql
SELECT * FROM t WHERE CAST(varchar_col AS TEXT) = 'hello';
-- If index is on varchar_col, TEXT cast may prevent index use
-- But removing cast changes comparison semantics
```

## References

**Implementation:**
- PostgreSQL: `eval_const_expressions()` removes redundant casts
- MySQL: Implicit cast optimization in comparisons
- DuckDB: Type resolution eliminates redundant casts
