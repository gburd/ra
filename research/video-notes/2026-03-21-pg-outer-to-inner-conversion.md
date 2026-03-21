# PostgreSQL: Outer-to-Inner Join Conversion

**Source:** PostgreSQL Planner Source Code Analysis
**Topic:** Converting outer joins to inner joins

## Key Concepts

### Why Convert Outer to Inner?
Inner joins allow:
1. Join commutativity: A INNER JOIN B = B INNER JOIN A
2. Join associativity: (A INNER B) INNER C = A INNER (B INNER C)
3. More join orderings: optimizer has larger search space
4. Predicate pushdown: predicates can move across inner joins freely
5. Better cost estimates: inner join cardinality easier to estimate

Outer joins restrict all of the above, making optimization harder.

### When Is Conversion Safe?
A LEFT JOIN can be converted to INNER JOIN when the WHERE clause
rejects NULL-extended rows from the nullable side.

**Null-rejecting predicates include:**
- `right.col IS NOT NULL` (explicit)
- `right.col = value` (equality rejects NULL)
- `right.col > value` (comparison rejects NULL)
- `right.col IN (...)` (IN rejects NULL)
- `right.col LIKE '...'` (LIKE rejects NULL)
- Any function where f(NULL) = NULL (strict functions)

### Examples

**Convertible:**
```sql
SELECT * FROM a LEFT JOIN b ON a.id = b.aid WHERE b.x > 5;
-- b.x > 5 rejects NULLs, so LEFT JOIN -> INNER JOIN
```

**NOT convertible:**
```sql
SELECT * FROM a LEFT JOIN b ON a.id = b.aid WHERE b.x > 5 OR b.x IS NULL;
-- OR with IS NULL preserves NULL rows
```

**Convertible (implicit):**
```sql
SELECT * FROM a
LEFT JOIN b ON a.id = b.aid
LEFT JOIN c ON b.cid = c.id
WHERE c.name = 'foo';
-- c.name = 'foo' rejects NULLs, so second LEFT -> INNER
-- Since b.cid must be non-NULL for c join, first LEFT -> INNER too
```

### Cascading Conversion
Converting an outer join can enable converting other outer joins.
This happens when:
1. A predicate on table C (through LEFT JOIN chain) implies
   intermediate tables must be non-NULL
2. Converting C's join may make B's join convertible
3. Process iterates until no more conversions possible

### Implementation in PostgreSQL
- Done in `reduce_outer_joins()` during preprocessing
- Walks plan tree bottom-up
- For each outer join, checks if nullable-side variables appear in
  strict conditions above the join
- Uses `clause_is_strict_for()` to test if predicate rejects NULLs
- After conversion, removes outer-join-specific relid sets

## Applicable to Ra

### New Rules
1. **Basic Outer-to-Inner**:
   ```
   Pattern: Filter(pred, Join(LeftOuter, A, B, cond))
   Condition: pred references B columns AND pred is null-rejecting
   Result: Filter(pred, Join(Inner, A, B, cond))
   ```

2. **Cascading Outer-to-Inner**:
   ```
   Pattern: Filter(pred, Join(LeftOuter, A,
              Join(LeftOuter, B, C, cond2), cond1))
   Condition: pred rejects NULLs on C columns
   Result: may convert both joins to INNER
   ```

3. **Right Outer to Inner**:
   ```
   Pattern: Filter(pred, Join(RightOuter, A, B, cond))
   Condition: pred references A columns AND pred is null-rejecting
   Result: Filter(pred, Join(Inner, A, B, cond))
   ```

4. **Full Outer Reduction**:
   ```
   Pattern: Filter(pred, Join(FullOuter, A, B, cond))
   Condition: pred rejects NULLs on A -> RIGHT OUTER
              pred rejects NULLs on B -> LEFT OUTER
              pred rejects NULLs on both -> INNER
   ```

### Prerequisites
- Null-rejection analysis for expressions
- Ability to determine which tables an expression references
- Understanding of strict functions (f(NULL) = NULL)

### Impact
- Enables join reordering that was previously blocked by outer joins
- Common in queries with LEFT JOINs followed by WHERE on nullable side
- Estimated: 10-20% of production queries with LEFT JOINs
