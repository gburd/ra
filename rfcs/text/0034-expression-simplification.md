# RFC 0034: Expression Simplification Extensions

- Start Date: 2026-03-21
- Author: RA Contributors
- Status: Accepted
- Tracking Issue: TBD

## Summary

Implement five expression-level optimizations: GROUP BY constant elimination, single DISTINCT to GROUP BY conversion, nested UNION flattening, cross join to inner conversion, and equijoin predicate extraction. Each is individually small but collectively significant for complex queries.

## Motivation

Several expression-level optimizations from DataFusion and other production systems are missing from RA. These pattern-matching rules catch common suboptimal patterns in SQL queries and rewrite them to more efficient forms. They are low-risk (well-understood transformations with clear correctness conditions) and provide cumulative benefit for complex queries.

## Guide-level explanation

### GROUP BY Constant Elimination

```sql
-- Before: constant in GROUP BY wastes hash/compare resources
SELECT 1, col1, col2, COUNT(*) FROM t GROUP BY 1, col1, col2;
-- After: constant removed
SELECT 1, col1, col2, COUNT(*) FROM t GROUP BY col1, col2;
```

### Single DISTINCT to GROUP BY

```sql
-- Before: sort-based DISTINCT elimination
SELECT COUNT(DISTINCT category) FROM products;
-- After: hash-based GROUP BY
SELECT COUNT(*) FROM (SELECT category FROM products GROUP BY category);
```

### Nested UNION Flattening

```sql
-- Before: nested union tree
(SELECT * FROM a UNION ALL SELECT * FROM b)
UNION ALL
SELECT * FROM c;
-- After: flat union
SELECT * FROM a UNION ALL SELECT * FROM b UNION ALL SELECT * FROM c;
```

### Cross Join to Inner Conversion

```sql
-- Before: cross join + filter
SELECT * FROM a, b WHERE a.id = b.aid;
-- After: inner join (enables hash/merge join selection)
SELECT * FROM a INNER JOIN b ON a.id = b.aid;
```

### Equijoin Predicate Extraction

```sql
-- Before: mixed join condition
SELECT * FROM a JOIN b ON a.id = b.aid AND a.val > b.val;
-- After: equality predicate separated for join method selection
-- equi: a.id = b.aid (enables hash join)
-- post-filter: a.val > b.val
```

## Reference-level explanation

### Implementation Details

**Rule: `group-by-constant-elimination`**
- Detect constant expressions in GROUP BY key list
- Remove them (constants do not affect grouping)

**Rule: `single-distinct-to-group-by`**
- Pattern: `Aggregate(COUNT(DISTINCT col), input)`
- Result: `Aggregate(COUNT(*), GroupBy(col, input))`
- Enables hash aggregation instead of sort-based distinct

**Rule: `flatten-nested-union`**
- Pattern: `Union(Union(A, B), C)` -> `Union(A, B, C)`
- Reduces plan tree depth for better optimization

**Rule: `cross-join-to-inner`**
- Pattern: `Filter(eq(a.col, b.col), CrossJoin(A, B))`
- Result: `InnerJoin(eq(a.col, b.col), A, B)`
- Enables join algorithm selection (hash join, merge join)

**Rule: `extract-equijoin-predicate`**
- Separate equality predicates from non-equality in join conditions
- Equality predicates determine join method eligibility
- Non-equality predicates applied as post-join filter

## Drawbacks

- Each rule adds a small amount of optimizer overhead
- Cross join to inner conversion changes semantics only when filter is an equijoin predicate (must be verified)
- DISTINCT to GROUP BY conversion may not benefit when the aggregation framework prefers sort-based distinct

## Rationale and alternatives

### Why This Design?

These are straightforward pattern-matching rules with clear correctness conditions. All are implemented in DataFusion and other production systems.

### Alternative Approaches

- **SQL-level rewriting**: Requires parser changes; RA operates on logical plans
- **Cost-based selection**: These transformations are universally beneficial; cost comparison is unnecessary
- **Larger rule composition**: Could combine with other rules, but keeping them separate aids maintainability

## Prior art

- DataFusion: `EliminateGroupByConstant`, `SingleDistinctToGroupBy`
- DataFusion: `EliminateCrossJoin`, `ExtractEquijoinPredicate`
- CockroachDB: `EliminateProject` and join normalization rules
- MySQL: implicit cross join to inner join conversion

## Unresolved questions

- Interaction between cross-join-to-inner and existing predicate pushdown rules
- Ordering of these rules relative to join reordering
- Handling of UNION DISTINCT flattening (requires duplicate elimination awareness)

## Future possibilities

- Subquery decorrelation rules
- Common subexpression elimination across UNION branches
- Predicate inference (transitivity closure) for additional filter pushdown
