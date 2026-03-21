# Rule: "Distributed Query Optimization"

**Category:** distributed/distributed-execution
**File:** `rules/database-specific/clickhouse/clickhouse-distributed-query-optimization.rra`

## Metadata

- **ID:** `clickhouse-distributed-query-optimization`
- **Version:** "1.0.0"
- **Databases:** clickhouse
- **Tags:** database-mining, clickhouse, optimization
- **Authors:** "Database Contributors", "RA Contributors"


# Distributed Query Optimization

## Description

Optimizes queries across distributed clusters by pushing down predicates and projections.

**When to apply**: During query optimization phase.

## Relational Algebra

```algebra
-- Before
(select * from distributed_table WHERE condition)

-- After
(distributed-select (select * from local_table WHERE condition))
```

## Preconditions

- Table is Distributed engine

## Test Cases

```sql
SELECT * FROM table WHERE condition;
```

## References

- Database documentation
