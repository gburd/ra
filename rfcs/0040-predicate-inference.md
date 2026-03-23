# RFC 0040: Predicate Inference and Transitivity Closure

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Proposed
- Tracking Issue: TBD

## Summary

Implement predicate inference through transitivity closure to derive additional filter predicates from existing join conditions and WHERE clauses, enabling more aggressive predicate pushdown and better cardinality estimation.

## Motivation

When a query contains `a.x = b.x AND b.x = c.x`, the predicate `a.x = c.x` can be inferred but RA does not derive it. Similarly, when `a.x = b.x AND a.x > 10`, the predicate `b.x > 10` can be inferred and pushed down to table `b`. Without predicate inference:
- Filter pushdown is limited to explicitly stated predicates
- Join reordering cannot exploit implicit equalities
- Cardinality estimates for downstream operators are less accurate

## Guide-level explanation

```sql
-- Original query
SELECT *
FROM orders o
JOIN customers c ON o.customer_id = c.id
JOIN regions r ON c.region_id = r.id
WHERE o.customer_id > 1000;

-- Inferred predicates:
-- c.id > 1000  (from o.customer_id = c.id AND o.customer_id > 1000)
-- This filter can be pushed down to the customers scan
```

The optimizer performs transitivity closure on equijoin predicates and range predicates to derive all implied filters, then pushes them to the earliest possible point in the plan.

## Reference-level explanation

### Implementation Details

**Equality transitivity**:
- Build equivalence classes from equijoin predicates
- For each equivalence class {a.x, b.x, c.x}, any predicate on one member applies to all

**Range predicate propagation**:
- If `a.x = b.x` and `a.x > 10`, infer `b.x > 10`
- If `a.x = b.x` and `a.x IN (1, 2, 3)`, infer `b.x IN (1, 2, 3)`
- If `a.x = b.x` and `a.x BETWEEN 1 AND 100`, infer `b.x BETWEEN 1 AND 100`

**Rules**:
- `predicate-transitivity-closure`: derive implied equality predicates
- `range-predicate-propagation`: derive implied range predicates
- `in-list-propagation`: derive implied IN predicates

### Integration Points

- Runs before predicate pushdown to maximize pushdown opportunities
- Feeds into cardinality estimation for tighter bounds
- Interacts with join reordering (new predicates may change optimal order)

## Drawbacks

- Equivalence class computation adds optimization overhead
- Derived predicates increase plan complexity
- Must avoid generating redundant predicates

## Rationale and alternatives

### Why This Design?

Transitivity closure is the standard approach used by PostgreSQL (EquivalenceClass system), CockroachDB, and Apache Calcite. It is well-understood and provably correct.

### Alternative Approaches

- **Manual predicate specification**: Requires user awareness of optimizer limitations
- **Join-time filtering only**: Misses early filtering opportunities

## Prior art

- PostgreSQL: EquivalenceClass system for transitive closure
- CockroachDB: filter-inference normalization rules
- Apache Calcite: `RelOptPredicateList` and transitive inference
- DB2: predicate transitive closure

## Unresolved questions

- Handling of NULL semantics in equivalence classes
- Interaction with outer joins (equivalences may not hold across outer joins)
- Maximum depth of inference chain before diminishing returns

## Future possibilities

- Functional dependency inference for additional pushdown
- Cross-query predicate caching for prepared statements
- Statistical inference (histogram intersection via equivalence classes)
