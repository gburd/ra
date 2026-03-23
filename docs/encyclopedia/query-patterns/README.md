# Query Patterns

Comprehensive catalog of SQL query patterns and how Ra optimizes them. Each pattern includes relational algebra, applicable transformation rules, and practical examples.

## Pattern Categories

### [OLTP Queries](oltp/)

Transaction processing patterns:
- [Point Lookups](oltp/point-lookup.md) - Single-row retrieval by primary key
- [Range Scans](oltp/range-scan.md) - Bounded index scans
- [Simple Updates](oltp/simple-update.md) - Single-row UPDATE/DELETE
- [Batch Inserts](oltp/batch-insert.md) - Multi-row INSERT optimization
- [Upserts](oltp/upsert.md) - INSERT ... ON CONFLICT UPDATE

### [OLAP Queries](olap/)

Analytical processing patterns:
- [Full Table Aggregation](olap/full-table-aggregation.md) - GROUP BY over entire table
- [Multi-level Grouping](olap/multi-level-grouping.md) - ROLLUP, CUBE, GROUPING SETS
- [Top-N Queries](olap/top-n.md) - ORDER BY LIMIT optimization
- [Distinct Aggregation](olap/distinct-aggregation.md) - COUNT(DISTINCT), multi-phase aggregation
- [Materialized Views](olap/materialized-views.md) - Incremental maintenance

### [Analytical Queries](analytical/)

Advanced analytics:
- [Window Functions](analytical/window-functions.md) - ROW_NUMBER, RANK, LAG, LEAD
- [Moving Aggregates](analytical/moving-aggregates.md) - Sliding windows
- [Percentiles](analytical/percentiles.md) - PERCENTILE_CONT, median calculation
- [Cumulative Sums](analytical/cumulative-sums.md) - Running totals
- [Pivot Tables](analytical/pivot-tables.md) - CASE-based pivoting

### [Recursive Queries](recursive/)

Recursive CTEs:
- [Transitive Closure](recursive/transitive-closure.md) - Graph reachability
- [Hierarchical Queries](recursive/hierarchical-queries.md) - Tree traversal
- [Path Enumeration](recursive/path-enumeration.md) - All paths between nodes
- [Bill of Materials](recursive/bill-of-materials.md) - Component explosion

### [Hierarchical Queries](hierarchical/)

Tree and graph patterns:
- [Parent-Child Queries](hierarchical/parent-child.md) - Adjacency list traversal
- [Nested Set Model](hierarchical/nested-set.md) - Modified pre-order traversal
- [Path Materialization](hierarchical/path-materialization.md) - Full path storage
- [Closure Tables](hierarchical/closure-tables.md) - Ancestor-descendant pairs

### [Temporal Queries](temporal/)

Time-based patterns:
- [Date Range Filters](temporal/date-range-filters.md) - Interval queries
- [Time Series Aggregation](temporal/time-series-aggregation.md) - Time buckets
- [Temporal Joins](temporal/temporal-joins.md) - AS OF, BETWEEN joins
- [Gap and Island Detection](temporal/gaps-and-islands.md) - Continuous sequences
- [Moving Time Windows](temporal/moving-time-windows.md) - Last N days/hours

### [Set Operations](set-operations/)

Set-based queries:
- [UNION](set-operations/union.md) - Set union with/without duplicates
- [INTERSECT](set-operations/intersect.md) - Set intersection
- [EXCEPT](set-operations/except.md) - Set difference
- [Anti-Join Patterns](set-operations/anti-join.md) - NOT EXISTS, NOT IN

### [Subqueries](subqueries/)

Nested query patterns:
- [Scalar Subqueries](subqueries/scalar-subquery.md) - Single value in SELECT
- [EXISTS Subqueries](subqueries/exists-subquery.md) - Semi-join optimization
- [IN Subqueries](subqueries/in-subquery.md) - Set membership
- [Correlated Subqueries](subqueries/correlated-subquery.md) - Dependent subqueries
- [Lateral Subqueries](subqueries/lateral-subquery.md) - LATERAL, CROSS APPLY

### [Joins](joins/)

Join patterns and strategies:
- [Inner Joins](joins/inner-join.md) - Equi-joins, theta-joins
- [Outer Joins](joins/outer-join.md) - LEFT, RIGHT, FULL OUTER
- [Cross Joins](joins/cross-join.md) - Cartesian products
- [Self Joins](joins/self-join.md) - Same table joins
- [Semi Joins](joins/semi-join.md) - EXISTS optimization
- [Anti Joins](joins/anti-join.md) - NOT EXISTS optimization
- [Lateral Joins](joins/lateral-join.md) - Dependent joins

## How to Use This Guide

### Finding Your Pattern

1. **Identify query structure** - Look at your SQL query's main operation
2. **Match to category** - Use the table of contents above
3. **Read pattern details** - Each pattern has:
   - Description and use cases
   - Relational algebra
   - Ra optimization rules
   - Statistics needed
   - Code examples

### Understanding Optimization

Each pattern doc explains:

**What Ra does automatically:**
- Rules that fire
- Transformations applied
- Cost model considerations

**What you can configure:**
- Statistics to provide
- Hints (if needed)
- Schema design tips

### Example Structure

```markdown
# Pattern Name

## Description
Plain English explanation.

## Use Cases
When you encounter this in practice.

## Relational Algebra
$$
\text{LaTeX notation}
$$

## How Ra Optimizes
- Specific rules that apply
- Transformations performed
- Cost considerations

## Statistics API
```rust
// Code showing how to provide stats
```

## Examples
```sql
-- Query examples
```

## See Also
Links to related patterns.
```

## Pattern Selection Guide

### By Query Type

| SQL Feature | Pattern Category |
|-------------|-----------------|
| `SELECT * FROM table WHERE id = ?` | [OLTP -> Point Lookup](oltp/point-lookup.md) |
| `SELECT COUNT(*), AVG(amount) FROM orders GROUP BY region` | [OLAP -> Full Table Aggregation](olap/full-table-aggregation.md) |
| `SELECT *, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY salary)` | [Analytical -> Window Functions](analytical/window-functions.md) |
| `WITH RECURSIVE cte AS (...) SELECT ...` | [Recursive -> Transitive Closure](recursive/transitive-closure.md) |
| `SELECT * FROM orders WHERE date BETWEEN ? AND ?` | [Temporal -> Date Range Filters](temporal/date-range-filters.md) |
| `SELECT * FROM a UNION SELECT * FROM b` | [Set Operations -> UNION](set-operations/union.md) |
| `SELECT * FROM orders WHERE customer_id IN (SELECT ...)` | [Subqueries -> IN Subquery](subqueries/in-subquery.md) |
| `SELECT * FROM a JOIN b ON a.id = b.a_id` | [Joins -> Inner Join](joins/inner-join.md) |

### By Performance Goal

| Goal | Pattern to Study |
|------|-----------------|
| Minimize I/O | [Point Lookups](oltp/point-lookup.md), [Range Scans](oltp/range-scan.md) |
| Reduce memory usage | [Top-N](olap/top-n.md), [Streaming Aggregation](olap/full-table-aggregation.md) |
| Parallelize computation | [Full Table Aggregation](olap/full-table-aggregation.md), [Partitioned Scans](../distributed-patterns/union-over-partitions.md) |
| Avoid large joins | [Subquery Unnesting](subqueries/correlated-subquery.md), [Semi Joins](joins/semi-join.md) |
| Speed up analytics | [Window Functions](analytical/window-functions.md), [Materialized Views](olap/materialized-views.md) |

## Cross-References

- [Schema Patterns](../schema-patterns/) - How schema design affects these patterns
- [Dataset Characteristics](../dataset-characteristics/) - Data properties impact on optimization
- [Distributed Patterns](../distributed-patterns/) - Multi-node variants
- [Index Structures](../index-structures/) - Index usage in patterns
- [Rules Documentation](../../rules/) - Specific transformation rules

## Contributing New Patterns

To add a pattern:

1. Create markdown file in appropriate subdirectory
2. Follow the template structure
3. Include complete relational algebra with LaTeX
4. Show Ra rules that apply (link to rule docs)
5. Provide working SQL examples
6. Cross-reference related patterns
7. Update this README

See [Contributing Guide](../../CONTRIBUTING.md) for standards.
