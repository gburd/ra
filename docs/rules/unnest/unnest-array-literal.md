# Rule: Unnest Array Literal to VALUES

**Category:** logical/unnest
**File:** `rules/unnest/unnest-array-literal.rra`

## Metadata

- **ID:** `unnest-array-literal-constant-fold`
- **Version:** "1.0.0"
- **Databases:** postgresql, duckdb, sqlite, mysql, oracle, mssql, generic
- **Tags:** unnest, constant-folding, array, values
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: pattern
    must_match: "(unnest (array ?elems))"
  - type: predicate
    condition: "is_constant_array(?elems)"
    description: "All array elements are compile-time constants"
```


# Unnest Array Literal to VALUES

## Description

When UNNEST operates on a constant array literal (all elements known
at plan time), expand the array into a VALUES clause at compile time.
This eliminates the unnest operator entirely and produces a simple
table scan over constant rows.

**When to apply**: The argument to UNNEST is an array constructor
whose every element is a constant (literal integer, string, etc.).

**Why it works**: `unnest(array[v1, v2, ..., vN])` is definitionally
equivalent to `VALUES (v1), (v2), ..., (vN)`. Constant-folding this
at plan time avoids runtime array construction and element extraction.

## Relational Algebra

```algebra
-- Before
unnest(array[1, 2, 3]) AS t(val)

-- After
VALUES (1), (2), (3) AS t(val)
```

### With ordinality

```algebra
-- Before
unnest(array['a', 'b', 'c']) WITH ORDINALITY AS t(val, ord)

-- After
VALUES ('a', 1), ('b', 2), ('c', 3) AS t(val, ord)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Constant-fold unnest of array literal to VALUES
rw!("unnest-array-literal";
    "(unnest (array ?elems))" =>
    "(values (expand-array ?elems))"
    if is_constant_array("?elems")
),

// Variant with ordinality: expand and number the rows
rw!("unnest-array-literal-with-ordinality";
    "(unnest-ord (array ?elems))" =>
    "(values (expand-array-with-ordinality ?elems))"
    if is_constant_array("?elems")
),
```

## Preconditions

```rust
fn is_constant_array(elems: &[Expr]) -> bool {
    // Every element must be a compile-time constant.
    // Column references, function calls, and subqueries
    // disqualify the array from constant folding.
    elems.iter().all(|e| matches!(e,
        Expr::Const(_)
    ))
}
```

**Restrictions:**
- All elements must be compile-time constants (no column refs, no function calls).
- Array must not be empty (empty unnest produces zero rows, which is valid but should be handled by a separate empty-unnest elimination rule).
- The array must fit in a reasonable size limit to avoid plan explosion (e.g., <= 1000 elements).

## Cost Model

```rust
fn estimated_benefit(array_length: usize) -> f64 {
    // Unnest operator cost: array construction + per-element
    // extraction + output buffering.
    let unnest_cost = array_length as f64 * 1.5;

    // VALUES scan cost: direct constant output, no array overhead.
    let values_cost = array_length as f64 * 1.0;

    // Additional benefit: VALUES enables further optimizations
    // (e.g., constant propagation, join with VALUES).
    (unnest_cost - values_cost) / unnest_cost
}
```

**Typical benefit**: 50-95%. The primary gain is not raw speed but
enabling downstream optimizations: a VALUES clause can participate
in constant propagation, join elimination, and predicate pushdown
that an unnest operator blocks.

## Test Cases

### Positive: integer array literal

```sql
-- Before
SELECT * FROM unnest(array[1, 2, 3]) AS t(val);

-- After (internal)
SELECT * FROM (VALUES (1), (2), (3)) AS t(val);
```

### Positive: string array literal

```sql
-- Before
SELECT * FROM unnest(array['alpha', 'beta', 'gamma']) AS t(name);

-- After (internal)
SELECT * FROM (VALUES ('alpha'), ('beta'), ('gamma')) AS t(name);
```

### Positive: used in join context

```sql
-- Before
SELECT e.name, t.tag
FROM employees e
JOIN unnest(array['senior', 'lead', 'staff']) AS t(tag) ON true;

-- After: VALUES can be hash-joined
SELECT e.name, t.tag
FROM employees e
JOIN (VALUES ('senior'), ('lead'), ('staff')) AS t(tag) ON true;
```

### Negative: array contains column reference

```sql
-- Cannot fold: array includes non-constant element
SELECT * FROM unnest(array[1, t.id, 3]) AS u(val);

-- t.id is not a compile-time constant.
```

### Negative: array from function call

```sql
-- Cannot fold: array is result of function, not literal
SELECT * FROM unnest(string_to_array('a,b,c', ',')) AS u(val);

-- string_to_array result is not known at compile time.
```

## References

- PostgreSQL: eval_const_expressions in optimizer/util/clauses.c
- DuckDB: ConstantFolding pass in optimizer
- Graefe, "The Cascades Framework for Query Optimization" (1995)
