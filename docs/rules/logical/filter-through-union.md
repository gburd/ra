# Rule: Filter Pushdown Through Union

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-through-union.rra`

## Metadata

- **ID:** `filter-through-union`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** filter, union, pushdown, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: "pattern"
    must_match: "(filter ?pred (union ?left ?right))"
    description: "Filter above a union"
  - type: "predicate"
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic (applied to both branches)"
```


# Filter Pushdown Through Union

## Description

Pushes a selection predicate through a UNION (or UNION ALL) operator so that
each branch of the union is filtered independently. This is always valid
because union concatenates rows and selection distributes over concatenation.

**When to apply**: A filter sits above a union and can be applied to both
branches independently.

**Why it works**: Filtering each branch before the union reduces the number
of rows passed to the union operator and any subsequent operators.

## Relational Algebra

```algebra
sigma[p](R union S) -> sigma[p](R) union sigma[p](S)
sigma[p](R union_all S) -> sigma[p](R) union_all sigma[p](S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("filter-through-union";
    "(filter ?pred (union ?left ?right))" =>
    "(union (filter ?pred ?left) (filter ?pred ?right))"
),

rw!("filter-through-union-all";
    "(filter ?pred (union_all ?left ?right))" =>
    "(union_all (filter ?pred ?left) (filter ?pred ?right))"
),
```

## Preconditions


> **Note:** Formal preconditions are defined in the YAML frontmatter above.

```rust
fn applicable(pred: &Expr) -> bool {
    // Always applicable: selection distributes over union.
    // The predicate references columns by position, which both
    // branches share by definition of UNION compatibility.
    true
}
```

**Restrictions:**
- Both branches must be union-compatible (guaranteed by SQL semantics)
- No additional preconditions required

## Cost Model

```rust
fn estimated_benefit(
    left_card: f64,
    right_card: f64,
    selectivity: f64,
) -> f64 {
    let total_before = left_card + right_card;
    let total_after =
        left_card * selectivity + right_card * selectivity;
    (total_before - total_after) / total_before
}
```

**Typical benefit**: Proportional to `(1 - selectivity)`. A highly
selective predicate (selectivity 0.01) yields ~99% reduction.

## Test Cases

```sql
-- Positive: push filter into both branches of UNION ALL
-- Before
SELECT * FROM (
    SELECT id, name FROM employees
    UNION ALL
    SELECT id, name FROM contractors
) t WHERE t.name LIKE 'A%';

-- After
SELECT * FROM (
    SELECT id, name FROM employees WHERE name LIKE 'A%'
    UNION ALL
    SELECT id, name FROM contractors WHERE name LIKE 'A%'
) t;
```

```sql
-- Expected: filter pushed into UNION branches
-- When filter-through-union is implemented, the predicate is
-- distributed to both branches of the UNION.
SELECT * FROM (
    SELECT city FROM customers
    UNION
    SELECT city FROM suppliers
) t WHERE t.city = 'NYC';
```

## References

PostgreSQL: src/backend/optimizer/prep/prepunion.c
DuckDB: src/optimizer/filter_pushdown.cpp - PushdownSetOperation()
MySQL: sql/sql_union.cc
Garcia-Molina et al. "Database Systems: The Complete Book" Section 16.2.3
