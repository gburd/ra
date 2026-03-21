# Schema Patterns

How different database schema designs affect query optimization. Ra adapts its strategies based on schema structure.

## Patterns

### [Star Schema](star-schema.md)
Central fact table with dimension tables. Optimized for OLAP queries.

### [Snowflake Schema](snowflake-schema.md)
Normalized dimensions extending star schema. Trade-off between space and query complexity.

### [Normalized (3NF/BCNF)](normalized.md)
Fully normalized with many small tables. Common in OLTP systems.

### [Denormalized](denormalized.md)
Wide tables with redundant data. Optimized for read performance.

### [Temporal Tables](temporal-tables.md)
Time-versioned data with history tracking (Type 2 SCD).

### [Partitioned Tables](partitioned-tables.md)
Horizontal partitioning by range, hash, or list.

### [Sharded Tables](sharded-tables.md)
Distributed tables across multiple nodes.

## Quick Reference

| Schema Type | Best For | Join Strategy | Aggregation |
|-------------|----------|--------------|-------------|
| Star | OLAP | Dimension-first, broadcast | Pushdown |
| Snowflake | Large dimensions | Multi-level joins | Pushdown |
| Normalized | OLTP, data integrity | Nested loop with indexes | Rare |
| Denormalized | Read-heavy | Minimal joins | Efficient |
| Temporal | Auditing, history | AS OF joins | Time-aware |
| Partitioned | Large tables | Partition pruning | Parallel |
| Sharded | Massive scale | Co-located joins | Distributed |
