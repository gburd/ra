# Rule: Push Projection Before Exchange

**Category:** distributed/filter-pushdown-distributed
**File:** `rules/distributed/filter-pushdown-distributed/project-before-exchange.rra`

## Metadata

- **ID:** `project-before-exchange`
- **Version:** "1.0.0"
- **Databases:** spark, presto, cockroachdb, citus
- **Tags:** distributed, projection, pushdown, exchange, column-pruning
- **Authors:** "RA Contributors"


# Push Projection Before Exchange

## Description

Push a column projection below the exchange to reduce the width of rows
transferred over the network. Only columns needed by downstream operators
should be shuffled.

## Relational Algebra

```algebra
Project[cols](Exchange[hash(k)](R))
  -> Exchange[hash(k)](Project[cols + k](R))
  where k subset of cols or k added for partitioning
```

## Test Cases

```sql
-- Test 1: Select few columns from wide table
SELECT o.id, o.total
FROM orders o              -- 50 columns, 256 bytes/row
JOIN customers c ON o.cid = c.id;
-- Expected: Project to (id, total, cid) before shuffle
-- (3 cols, ~24 bytes vs 50 cols, 256 bytes)
```

```sql
-- Test 2: Preserve partition key in projection
SELECT o.total
FROM orders o
DISTRIBUTE BY o.customer_id;
-- Expected: Project to (total, customer_id) - keep partition key
```

```sql
-- Test 3: All columns needed
SELECT *
FROM orders o
JOIN customers c ON o.cid = c.id;
-- Expected: No projection pushdown (all columns used)
```

## References

Spark: ColumnPruning.scala
Presto: PruneRedundantProjectionAssignments
