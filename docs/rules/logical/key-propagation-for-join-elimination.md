# Rule: Key Propagation for Join Elimination

**Category:** logical/join-elimination
**File:** `rules/logical/join-elimination/key-propagation-for-join-elimination.rra`

## Metadata

- **ID:** `key-propagation-for-join-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, oracle, mssql, duckdb
- **Tags:** join, elimination, key-propagation, uniqueness, functional-dependency
- **Authors:** "RA Contributors"


# Key Propagation for Join Elimination

## Description

Propagates uniqueness constraints through joins and projections to enable
join elimination that would otherwise be invisible. When a join equates
columns, uniqueness on one side implies uniqueness on the equated column in
the result. Tracking this propagation lets the optimizer discover that
downstream joins are redundant.

**When to apply**: Multi-join queries where intermediate results gain
uniqueness properties through equi-join predicates, enabling elimination
of later joins.

**Why it works**: If R.a is unique and the join condition is R.a = S.b,
then S.b is functionally determined in the join result. If a later join
on S.b = T.c only needs T columns already determined by S.b, the join
with T can be eliminated.

## Relational Algebra

```algebra
Given:
  J1 = join[R.pk = S.fk](R, S)  -- R.pk is unique
  J2 = join[J1.fk2 = T.pk](J1, T) -- J1.fk2 unique via propagation

If J1 propagates uniqueness of R.pk to S.fk in the result,
and S.fk determines S.fk2 (via functional dependency),
then J2 can be eliminated if no T columns are needed.

project[cols_from_J1](join[J1.key = T.pk](J1, T))
  -> project[cols_from_J1](J1)
  where key is unique in J1 (via propagation)
  where T.pk is unique
  where no columns from T needed
```

## Implementation

```rust
use egg::{rewrite as rw, *};

// Propagate uniqueness through equi-join
rw!("propagate-uniqueness-through-join";
    "(join (= ?a ?b) ?left ?right)" =>
    { PropagateUniqueness {
        left: "?left", right: "?right",
        left_col: "?a", right_col: "?b",
    }}
),

struct PropagateUniqueness { /* ... */ }

impl Applier for PropagateUniqueness {
    fn apply(&self, egraph: &mut EGraph, matched: &SearchMatches) {
        // If ?a is unique in ?left, mark ?b as unique in join result
        if has_unique_key(egraph, self.left, self.left_col) {
            mark_unique_in_result(egraph, matched, self.right_col);
        }
        // Symmetric: if ?b is unique in ?right, mark ?a
        if has_unique_key(egraph, self.right, self.right_col) {
            mark_unique_in_result(egraph, matched, self.left_col);
        }
    }
}

// After propagation, standard join elimination rules apply
rw!("eliminate-join-via-propagated-key";
    "(project ?cols (join (= ?fk ?pk) ?input ?table))" =>
    "(project ?cols ?input)"
    if is_propagated_unique("?input", "?fk")
    if is_unique_key_table("?table", "?pk")
    if no_columns_from("?cols", "?table")
),
```

**Restrictions:**
- Requires tracking uniqueness metadata through the plan tree
- Outer joins do not propagate uniqueness from the nullable side
- GROUP BY introduces new uniqueness on grouping columns
- DISTINCT introduces uniqueness on all projected columns

## Cost Model

```rust
fn estimated_benefit(joins_eliminated: usize, table_sizes: &[u64]) -> f64 {
    // Each eliminated join saves its full execution cost
    let total_join_cost: f64 = table_sizes.iter()
        .map(|&s| s as f64 * 2.0) // build + probe per join
        .sum();

    let eliminated_cost: f64 = table_sizes[..joins_eliminated].iter()
        .map(|&s| s as f64 * 2.0)
        .sum();

    eliminated_cost / total_join_cost
}
```

**Typical benefit**: 50-90% in star/snowflake schemas where dimension
chains can be traced through propagated keys

## Test Cases

### Positive: Snowflake schema key propagation

```sql
-- Schema: orders -> customers -> regions (FK chain)
-- orders.customer_id -> customers.id (PK)
-- customers.region_id -> regions.id (PK)
SELECT o.id, o.amount
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN regions r ON c.region_id = r.id;

-- c.id is unique, propagates to o.customer_id in J1
-- c.region_id is determined by c.id
-- r.id is unique, no r columns needed -> eliminate regions join
-- Then: c.id unique, no c columns needed -> eliminate customers join
-- Result: SELECT o.id, o.amount FROM orders o;
```

### Positive: Self-referencing hierarchy with propagation

```sql
SELECT e.name
FROM employees e
JOIN employees m ON e.manager_id = m.id
JOIN departments d ON m.dept_id = d.id;

-- m.id unique -> propagates uniqueness
-- No d columns needed -> eliminate departments join
-- No m columns needed -> eliminate manager join
-- Result: SELECT e.name FROM employees e;
```

### Positive: GROUP BY creates uniqueness for later elimination

```sql
SELECT s.product_id, s.total_qty
FROM (
  SELECT product_id, SUM(qty) AS total_qty
  FROM sales GROUP BY product_id
) s
JOIN products p ON s.product_id = p.id;

-- GROUP BY product_id makes product_id unique in s
-- p.id is PK, no p columns needed -> eliminate join
```

### Negative: Outer join blocks propagation from nullable side

```sql
SELECT o.id
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
JOIN regions r ON c.region_id = r.id;

-- LEFT JOIN: c.region_id can be NULL for unmatched rows
-- Cannot propagate uniqueness through NULL values
-- regions join cannot be eliminated
```

### Negative: Non-equi join blocks propagation

```sql
SELECT a.id
FROM table_a a
JOIN table_b b ON a.value BETWEEN b.low AND b.high
JOIN table_c c ON b.id = c.b_id;

-- Range join does not propagate uniqueness
-- Cannot eliminate table_c join via this rule
```

## References

**Academic papers:**
- Galindo-Legaria & Rosenthal, "Outerjoin Simplification and Reordering", ACM TODS 1997
- Paulley, "Exploiting Functional Dependence in Query Optimization", PhD Thesis, U. Waterloo 2001
- Simmen et al., "Fundamental Techniques for Order Optimization", SIGMOD 1996

**Implementation:**
- PostgreSQL: `build_joinrel_tlist()` uniqueness tracking
- Oracle: Key-preserved table analysis in join elimination
- mssql: Functional dependency tracking in cardinality estimation
- DuckDB: Column binding metadata propagation
