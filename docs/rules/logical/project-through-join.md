# Rule: Projection Pushdown Through Join

**Category:** logical/projection-pushdown
**File:** `rules/logical/projection-pushdown/project-through-join.rra`

## Metadata

- **ID:** `project-through-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, duckdb, sqlite, oracle, mssql
- **Tags:** projection, join, pushdown, core
- **SQL Standard:** "sql:1992"
- **Authors:** "RA Contributors"


# Projection Pushdown Through Join

## Description

Pushes a projection through a join to reduce the width of intermediate
results. Before the join, each input is projected to only the columns
needed by the join condition and the final output. Narrower tuples reduce
memory consumption, cache pressure, and I/O cost.

**When to apply**: A projection above a join requests fewer columns than
the join's inputs produce.

**Why it works**: Columns not needed by the join condition or the final
output are dead after the join and can be pruned before the join executes.

## Relational Algebra

```algebra
pi[A](R join[c] S) -> pi[A]((pi[A_R union attrs(c)_R](R)) join[c] (pi[A_S union attrs(c)_S](S)))
  where A_R = A intersect attrs(R)
  where A_S = A intersect attrs(S)
  where attrs(c)_R = attrs(c) intersect attrs(R)
  where attrs(c)_S = attrs(c) intersect attrs(S)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("project-through-join";
    "(project ?cols (join ?kind ?cond ?left ?right))" =>
    "(project ?cols
        (join ?kind ?cond
            (project ?left_needed ?left)
            (project ?right_needed ?right)))"
    if can_push_project("?cols", "?cond", "?left", "?right")
),
```

## Preconditions

```rust
fn applicable(
    output_cols: &[Column],
    join_cond: &Expr,
    left: &Relation,
    right: &Relation,
) -> bool {
    // There must be columns in left or right that are NOT needed
    // (i.e., pruning actually removes something)
    let needed = output_cols.iter().chain(
        join_cond.referenced_columns().iter()
    ).collect::<HashSet<_>>();

    let left_available = left.output_columns();
    let right_available = right.output_columns();

    // At least one column can be pruned from either side
    left_available.iter().any(|c| !needed.contains(c))
        || right_available.iter().any(|c| !needed.contains(c))
}
```

**Restrictions:**
- Must preserve all columns needed by the join condition
- Must preserve all columns needed by the final output
- For outer joins, must also preserve columns from the nullable side
  that are needed to produce NULL markers

## Cost Model

```rust
fn estimated_benefit(
    left_card: f64,
    right_card: f64,
    cols_pruned: usize,
    total_cols: usize,
) -> f64 {
    // Narrower tuples reduce memory and I/O proportionally
    let width_reduction =
        cols_pruned as f64 / total_cols as f64;
    let join_result_card = left_card * right_card;
    width_reduction * join_result_card * TUPLE_WIDTH_COST
}
```

**Typical benefit**: 0.1-0.6 depending on how many columns are pruned.
Wide fact tables (50+ columns) benefit most.

## Test Cases

```sql
-- Positive: prune unused columns before join
-- Before
SELECT o.id, c.name
FROM orders o  -- has 20 columns
JOIN customers c ON o.customer_id = c.id;  -- has 15 columns

-- After (only needed columns survive)
-- Left input: project[id, customer_id](orders)  -- 2 columns
-- Right input: project[id, name](customers)  -- 2 columns
-- Then join, then project[o.id, c.name]
```

```sql
-- Positive: star query with projection
-- Before
SELECT o.total FROM orders o
JOIN items i ON o.id = i.order_id
JOIN products p ON i.product_id = p.id
WHERE p.category = 'electronics';

-- After (each input projected to minimum needed columns)
```

```sql
-- Negative: all columns needed
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id;
-- SELECT * needs all columns, nothing to prune
```

## References

PostgreSQL: src/backend/optimizer/path/allpaths.c - set_rel_pathlist()
DuckDB: src/optimizer/remove_unused_columns.cpp
MySQL: sql/sql_resolver.cc
Chaudhuri & Shim "Including Group-By in Query Optimization" (VLDB 1994)
