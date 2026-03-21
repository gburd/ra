# Rule: Apache Derby View Flattening

**Category:** database-specific/derby
**File:** `rules/database-specific/derby/view-flattening.rra`

## Metadata

- **ID:** `derby-view-flattening`
- **Version:** "1.0.0"
- **Databases:** derby
- **Tags:** database-specific, derby, view, flattening, inline, merge
- **Authors:** "RA Contributors"


# Apache Derby View Flattening

## Description

Derby flattens (inlines) views and derived tables into the outer
query, replacing the view reference with the view's underlying query.
This allows the optimizer to consider all tables together for join
ordering and enables predicate pushdown through the view boundary.

**When to apply**: A view or derived table can be safely merged into
the containing query without changing semantics.

**Why it works**: Without flattening, Derby treats the view as an
opaque subquery, possibly materializing it.  Flattening exposes the
view's tables to the outer optimizer, enabling cross-view predicate
pushdown and join reordering.

**Database version**: Apache Derby 10.1+

## Relational Algebra

```algebra
-- Before: opaque view reference
orders join view_active_customers

-- After: flattened
orders join sigma[active = true](customers)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("derby-view-flattening";
    "(join ?pred ?outer (view ?view_def))" =>
    "(join ?pred ?outer ?view_def)"
    if is_database("derby")
    if is_flattenable("?view_def")
),
```

## Preconditions

```rust
fn applicable(view: &ViewDef) -> bool {
    !view.has_aggregation()
    && !view.has_distinct()
    && !view.has_union()
    && !view.has_limit()
}
```

**Restrictions:**
- Cannot flatten views with GROUP BY, HAVING, DISTINCT, or UNION
- Cannot flatten views with LIMIT/OFFSET
- Recursive views cannot be flattened

## Cost Model

```rust
fn estimated_benefit(
    _rows: f64,
) -> f64 {
    0.0 // Enables further optimizations; no direct cost reduction
}
```

**Typical benefit**: Enables predicate pushdown and join reordering
across view boundaries.

## Test Cases

```sql
-- Positive: simple view flattened
CREATE VIEW active_custs AS
    SELECT * FROM customers WHERE active = true;
SELECT * FROM orders o JOIN active_custs c ON o.cust_id = c.id;
-- Flattened: orders JOIN customers WHERE active = true
```

```sql
-- Negative: view with aggregation
CREATE VIEW dept_stats AS
    SELECT dept, AVG(sal) avg_sal FROM emp GROUP BY dept;
SELECT * FROM dept_stats WHERE avg_sal > 50000;
-- Cannot flatten due to GROUP BY
```

## References

Apache Derby: Optimizer documentation, "View Flattening"
Source: org.apache.derby.impl.sql.compile.FromSubquery,
  `flattenFromSubquery()`
