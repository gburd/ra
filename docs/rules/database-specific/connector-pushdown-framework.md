# Rule: Connector Pushdown Framework (Trino)

**Category:** database-specific/trino
**File:** `rules/database-specific/trino/connector-pushdown-framework.rra`

## Metadata

- **ID:** `trino-connector-pushdown-framework`
- **Version:** "1.0.0"
- **Databases:** trino
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Connector Pushdown Framework (Trino)

## Metadata
- **Rule ID**: `trino-connector-pushdown`
- **Category**: Database-Specific / Trino
- **Source**: Trino ConnectorMetadata

## Description

Trino's pluggable connector framework allows pushing filters, projections, aggregations, joins, and limits into underlying data sources (PostgreSQL, MySQL, Elasticsearch, etc.).

**Pushdown capabilities (connector-specific):**
- Filter pushdown
- Column pruning (projection pushdown)
- Limit pushdown
- TopN pushdown
- Aggregate pushdown
- Join pushdown (connector-to-connector)

## Implementation Pattern

```java
// Trino ConnectorMetadata interface
public interface ConnectorMetadata {
    // Filter pushdown
    Optional<ConstraintApplicationResult<TableHandle>> applyFilter(
        ConnectorSession session,
        TableHandle table,
        Constraint constraint);

    // Projection pushdown
    Optional<ProjectionApplicationResult<TableHandle>> applyProjection(
        ConnectorSession session,
        TableHandle table,
        List<ConnectorExpression> projections);

    // Aggregation pushdown
    Optional<AggregationApplicationResult<TableHandle>> applyAggregation(
        ConnectorSession session,
        TableHandle table,
        List<AggregateFunction> aggregations,
        Map<String, ColumnHandle> assignments,
        List<List<ColumnHandle>> groupingSets);

    // TopN pushdown
    Optional<TopNApplicationResult<TableHandle>> applyTopN(
        ConnectorSession session,
        TableHandle table,
        long topNCount,
        List<SortItem> sortItems);
}
```

## Test Cases

### Test 1: Filter pushdown to PostgreSQL
```sql
-- Trino federates across PostgreSQL and MySQL
SELECT *
FROM postgresql.schema.users u
JOIN mysql.schema.orders o ON u.id = o.user_id
WHERE u.age > 25 AND o.status = 'completed';

-- Optimization:
-- 1. Push "age > 25" to PostgreSQL connector
-- 2. PostgreSQL executes: SELECT * FROM users WHERE age > 25
-- 3. Push "status = 'completed'" to MySQL connector
-- 4. MySQL executes: SELECT * FROM orders WHERE status = 'completed'
-- 5. Trino joins filtered results

-- Without pushdown: Transfer all users + all orders across network
-- With pushdown: Transfer only filtered rows (10-100x reduction)
```

### Test 2: Aggregate pushdown to MongoDB
```sql
SELECT category, COUNT(*), AVG(price)
FROM mongodb.catalog.products
GROUP BY category;

-- Pushdown to MongoDB:
-- db.products.aggregate([
--   { $group: {
--       _id: "$category",
--       count: { $sum: 1 },
--       avg_price: { $avg: "$price" }
--   }}
-- ])

-- Aggregation computed in MongoDB, only group results transferred
```

### Test 3: Join pushdown (experimental)
```sql
-- Both tables in same PostgreSQL database
SELECT *
FROM postgresql.db1.users u
JOIN postgresql.db1.orders o ON u.id = o.user_id;

-- Pushdown entire join to PostgreSQL:
-- SELECT * FROM users u JOIN orders o ON u.id = o.user_id

-- No data transfer to Trino until final results
```

## References

1. **Trino Docs**: "Connector Development"
   - https://trino.io/docs/current/develop/connectors.html

2. **Trino Source**: ConnectorMetadata.java
   - https://github.com/trinodb/trino/blob/master/core/trino-spi/src/main/java/io/trino/spi/connector/ConnectorMetadata.java

## Tags
`database-specific`, `trino`, `connector`, `pushdown`, `federation`, `polystore`
