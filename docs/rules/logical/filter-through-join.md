# Rule: Filter Pushdown Through Join

**Category:** logical/predicate-pushdown
**File:** `rules/logical/predicate-pushdown/filter-through-join.rra`

## Metadata

- **ID:** `filter-through-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** filter, join, pushdown, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"

## Preconditions

```yaml
  - type: pattern
    must_match: "(filter ?pred (join inner ?cond ?left ?right))"
    description: "Filter above an inner join"
  - type: predicate
    condition: "is_deterministic(?pred)"
    description: "Predicate must be deterministic (no random(), now(), etc.)"
  - type: predicate
    condition: "references_only(?pred, ?left) OR references_only(?pred, ?right)"
    description: "Predicate must reference columns from only one side of join"
```


# Filter Pushdown Through Join

## Description

Pushes selection predicates through join operators when the predicate only
references columns from one side of the join. This reduces the number of
tuples that participate in the join, which is typically the most expensive
relational operator.

**When to apply**: A filter sits directly above a join and the filter
predicate references columns from only the left or only the right input.

**Why it works**: Filtering before joining reduces the cardinality of one
input, shrinking the intermediate join result and lowering I/O and CPU cost.

## Relational Algebra

```algebra
sigma[p](R join[c] S) -> (sigma[p](R)) join[c] S
  where attrs(p) subset attrs(R)

sigma[p](R join[c] S) -> R join[c] (sigma[p](S))
  where attrs(p) subset attrs(S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("filter-through-join-left";
    "(filter ?pred (join inner ?cond ?left ?right))" =>
    "(join inner ?cond (filter ?pred ?left) ?right)"
    if references_only("?pred", "?left")
),

rw!("filter-through-join-right";
    "(filter ?pred (join inner ?cond ?left ?right))" =>
    "(join inner ?cond ?left (filter ?pred ?right))"
    if references_only("?pred", "?right")
),
```

## Preconditions

```rust
fn applicable(join_type: JoinType, pred: &Expr) -> bool {
    // Only safe for INNER joins without modification
    if !matches!(join_type, JoinType::Inner) {
        return false;
    }
    // Predicate must be deterministic
    if !pred.is_deterministic() {
        return false;
    }
    // Predicate must reference only one side
    let refs = pred.referenced_columns();
    refs.is_subset(&left.output_columns())
        || refs.is_subset(&right.output_columns())
}
```

**Restrictions:**
- Only applies to INNER joins (not LEFT/RIGHT/FULL OUTER)
- Predicate must be deterministic (no `random()`, `now()`, etc.)
- Predicate must reference columns from only one side of the join

## Cost Model

```rust
fn estimated_benefit(
    left_card: f64,
    right_card: f64,
    selectivity: f64,
) -> f64 {
    let cost_before = left_card * right_card;
    let filtered_card = left_card * selectivity;
    let cost_after = filtered_card * right_card;
    (cost_before - cost_after) / cost_before
}
```

**Typical benefit**: 0.5-0.99 depending on predicate selectivity.

## Test Cases

```sql
-- Positive: basic filter pushdown to left side
-- Before
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > 1000;

-- After
SELECT * FROM (
    SELECT * FROM orders WHERE amount > 1000
) o
JOIN customers c ON o.customer_id = c.id;
```

```sql
-- Positive: filter pushdown to right side
-- Before
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'US';

-- After
SELECT * FROM orders o
JOIN (
    SELECT * FROM customers WHERE country = 'US'
) c ON o.customer_id = c.id;
```

```sql
-- Expected: cross-reference predicate stays above join
-- Predicate references both sides so filter-through-join does not apply,
-- but other optimizer rules may still transform the plan.
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.amount > c.credit_limit;
```

```sql
-- Expected: LEFT JOIN predicate not pushed to right side
-- Pushing c.country = 'US' below the LEFT JOIN would change semantics,
-- but the optimizer may convert this to an inner join or apply other rules.
SELECT * FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.country = 'US';
```

## References

PostgreSQL: src/backend/optimizer/plan/initsplan.c - distribute_restrictinfo_to_rels()
MySQL: sql/sql_optimizer.cc - make_join_select()
DuckDB: src/optimizer/filter_pushdown.cpp
Selinger et al. "Access Path Selection in a Relational DBMS" (SIGMOD 1979)
Garcia-Molina et al. "Database Systems: The Complete Book" Section 16.2
