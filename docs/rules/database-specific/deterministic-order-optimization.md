# Rule: Deterministic Order Optimization (VoltDB)

**Category:** database-specific/voltdb
**File:** `rules/database-specific/voltdb/deterministic-order-optimization.rra`

## Metadata

- **ID:** `voltdb-deterministic-order-optimization`
- **Version:** "1.0.0"
- **Databases:** voltdb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Deterministic Order Optimization (VoltDB)

## Metadata
- **Rule ID**: `voltdb-deterministic-order`
- **Category**: Database-Specific / VoltDB
- **Source**: VoltDB
- **Docs**: https://docs.voltdb.com/

## Description

VoltDB enforces deterministic query execution for replication consistency. Automatically adds ORDER BY to non-deterministic queries to ensure replica consistency.

**Requirement**: All queries must produce identical results on all replicas.

## Relational Algebra

```
SELECT * FROM R WHERE p
→ SELECT * FROM R WHERE p ORDER BY primary_key

// Ensures deterministic result ordering
```

## Implementation Pattern

```java
// VoltDB PlanAssembler (conceptual)
public AbstractPlanNode compile(ParsedSelectStmt stmt) {
    AbstractPlanNode plan = buildInitialPlan(stmt);

    // Check if result order is deterministic
    if (\!hasOrderBy(stmt) && \!isSingleRow(plan)) {
        // Add implicit ORDER BY primary key
        OrderByPlanNode orderBy = new OrderByPlanNode();
        orderBy.addSortExpression(getPrimaryKeyColumns(stmt.table));
        plan = orderBy.addChild(plan);
    }

    return plan;
}
```

## Test Cases

### Test 1: Implicit ORDER BY for replication
```sql
-- User query (non-deterministic without ORDER BY)
SELECT * FROM users WHERE age > 25 LIMIT 10;

-- VoltDB adds implicit ORDER BY
SELECT * FROM users WHERE age > 25 ORDER BY user_id LIMIT 10;

-- Ensures all replicas return same 10 rows
```

### Test 2: Determinism check
```sql
-- This is deterministic (single row)
SELECT * FROM users WHERE user_id = 123;
-- No ORDER BY needed

-- This is deterministic (explicit ORDER BY)
SELECT * FROM users ORDER BY created_at DESC LIMIT 10;
-- No change needed

-- This is non-deterministic
SELECT * FROM users WHERE status = 'active';
-- Implicitly becomes:
-- SELECT * FROM users WHERE status = 'active' ORDER BY user_id;
```

## References

1. **VoltDB Docs**: "Deterministic Execution"
2. **Paper**: "H-Store: A High-Performance, Distributed Main Memory Transaction Processing System" (VLDB 2008)

## Tags
`database-specific`, `voltdb`, `deterministic`, `replication`, `consistency`, `in-memory`
