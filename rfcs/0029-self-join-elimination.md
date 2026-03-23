# RFC 0029: Self-Join Elimination and Outer-to-Inner Conversion

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Detect and eliminate self-joins where a table is joined to itself on primary/unique key unnecessarily, and convert outer joins to inner joins when WHERE clauses make the outer behavior unnecessary. Both transformations are common in ORM-generated queries.

## Motivation

Self-joins on unique keys are redundant: joining a table to itself on its primary key produces the same rows. This pattern is common in ORM-generated SQL (Django, Rails, SQLAlchemy) where query builders merge multiple query fragments. Similarly, LEFT JOINs are often followed by WHERE clauses that reject NULL-extended rows, making the outer join semantics unnecessary.

These patterns affect 5-15% of production queries, and eliminating them can halve execution time by removing entire join operations.

## Guide-level explanation

### Self-Join Elimination

```sql
-- ORM generates this when combining two query fragments
SELECT t1.id, t1.name, t2.email
FROM users t1
JOIN users t2 ON t1.id = t2.id;
-- Simplified to: SELECT id, name, email FROM users;
```

### Outer-to-Inner Conversion

```sql
-- LEFT JOIN with null-rejecting WHERE
SELECT o.id, c.name
FROM orders o
LEFT JOIN customers c ON o.customer_id = c.id
WHERE c.active = true;  -- rejects NULLs, so LEFT JOIN = INNER JOIN
```

### Full Outer Join Reduction

```sql
-- FULL JOIN reducible when one side has null-rejecting predicate
SELECT * FROM a FULL JOIN b ON a.id = b.aid
WHERE a.x > 0;  -- rejects NULL a-side, so FULL -> RIGHT
```

## Reference-level explanation

### Implementation Details

**Self-Join Elimination**:
- Detect when same table appears on both sides of INNER JOIN
- Verify join is on unique/primary key columns
- Verify no conflicting column references between the two copies
- Rule: `self-join-elimination`

**Outer-to-Inner Join Conversion**:
- Detect NULL-rejecting predicates on the nullable side of outer join
- NULL-rejecting predicates: equality, comparison, IS NOT NULL, strict functions
- LEFT JOIN -> INNER JOIN when predicate rejects NULLs on right side
- Rule: `outer-to-inner-join-conversion`
- Note: conversion is cascading (converting one join may enable others)

**Full Outer Join Reduction**:
- FULL -> LEFT when predicate rejects NULLs on right side
- FULL -> RIGHT when predicate rejects NULLs on left side
- FULL -> INNER when predicates reject NULLs on both sides
- Rule: `full-outer-join-reduction`

```rust
fn is_null_rejecting(predicate: &Expr, table: &TableRef) -> bool {
    match predicate {
        Expr::BinaryOp { left, right, .. } => {
            references_table(left, table) || references_table(right, table)
        }
        Expr::IsNotNull(col) => references_table(col, table),
        Expr::Function { strict: true, args, .. } => {
            args.iter().any(|a| references_table(a, table))
        }
        _ => false,
    }
}
```

## Drawbacks

- Self-join elimination requires unique key metadata (not always available)
- Null-rejection analysis must be conservative (false negatives are safe, false positives are bugs)
- ORM query patterns vary; coverage is workload-dependent

## Rationale and alternatives

### Why This Design?

Both transformations are well-understood with clear correctness conditions. They remove entire operations from the plan rather than just optimizing them, providing large speedups with low implementation risk.

### Alternative Approaches

- **Query rewriting in the ORM layer**: Requires per-ORM support
- **View merging**: Addresses some cases but not all
- **Materialized subquery elimination**: More general but more complex

## Prior art

- PostgreSQL v17: `enable_self_join_elimination`
- TiDB: outer-join-elimination rule
- CockroachDB: `EliminateJoin` transformation
- DataFusion: `EliminateOuterJoin` rule
- MySQL: outer-join simplification in the resolver

## Unresolved questions

- Handling of self-joins with different column subsets
- Multi-way self-join detection
- Interaction with join reordering

## Future possibilities

- Semi-join to scalar subquery conversion
- Anti-join simplification with NOT EXISTS patterns
- Detection of redundant joins in complex CTEs
