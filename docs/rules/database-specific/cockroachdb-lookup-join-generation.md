# Rule: "CockroachDB Lookup Join Generation"

**Category:** physical/join-selection
**File:** `rules/database-specific/cockroachdb/cockroachdb-lookup-join-generation.rra`

## Metadata

- **ID:** `cockroachdb-lookup-join-generation`
- **Version:** "1.0.0"
- **Databases:** cockroachdb
- **Tags:** cockroachdb, join, lookup-join, physical, xform, index
- **Authors:** "Cockroach Labs", "RA Contributors"


# CockroachDB Lookup Join Generation

## Description

CockroachDB's GenerateLookupJoins function creates lookup join execution plans
when the ON condition constraints specific index columns to constant or range
values. A lookup join treats one relation as a parameterized inner loop, using
values from the outer relation to probe an index on the inner relation. This
is highly efficient for foreign key joins and selective lookups.

The rule examines whether the ON condition constrains index column prefixes
to non-ranging constant values or already-specified ranges, making the index
directly applicable.

**When to apply**: After join reordering, when one side has a suitable index
on columns constrained by the ON condition.

## Relational Algebra

```algebra
-- Before: generic inner join
(inner-join
  (scan orders)
  (scan customers)
  (eq order.customer_id customer.id))

-- After: lookup join using index on customers.id
(lookup-join
  (scan orders)
  (index-scan customers idx_pk_id (const_value order.customer_id))
  (eq order.customer_id customer.id))
```

## Implementation

```rust
fn generate_lookup_joins(
    join_expr: &Expr,
    left: &Expr,
    right: &Expr,
    on_condition: &Expr,
) -> Vec<Expr> {
    // Check if right side has index on join columns
    // Verify ON condition constrains index prefix
    // Generate LookupJoin operator
}
```

## Preconditions

```
- Inner relation has an index on the join columns
- ON condition references indexed columns
- Index columns are constrained to non-ranging values
- Selectivity favors index lookup over full scan
```

## Cost Model

Lookup join cost = outer_cardinality $\times$ (index_lookup_cost + inner_filter_cost)

Typical index lookup cost: 1-10x cheaper than nested loop join for selective predicates.

## Test Cases

```sql
-- Positive: selective lookup on indexed foreign key
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE o.status = 'pending';
-- Index on customers(id) enables lookup join

-- Negative: no suitable index
SELECT * FROM orders o
JOIN customers c ON o.region = c.region
WHERE o.status = 'pending';
-- No index on region, falls back to hash/nested loop

-- Negative: range predicate
SELECT * FROM orders o
JOIN customers c ON o.created_at BETWEEN c.start_date AND c.end_date
WHERE o.status = 'pending';
-- Range constraint prevents simple lookup join
```

## References

- CockroachDB: pkg/sql/opt/xform/join_funcs.go
- PostgreSQL: Nested Loop Join with Index
