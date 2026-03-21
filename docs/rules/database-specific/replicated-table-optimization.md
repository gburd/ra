# Rule: Replicated Table Optimization (VoltDB)

**Category:** database-specific/voltdb
**File:** `rules/database-specific/voltdb/replicated-table-optimization.rra`

## Metadata

- **ID:** `voltdb-replicated-table-optimization`
- **Version:** "1.0.0"
- **Databases:** voltdb
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Replicated Table Optimization (VoltDB)

## Metadata
- **Rule ID**: `voltdb-replicated-table`
- **Category**: Database-Specific / VoltDB
- **Source**: VoltDB

## Description

VoltDB replicates small dimension tables to all partitions, enabling local joins without network traffic.

## Test Cases

### Test 1: Replicated dimension join
```sql
CREATE TABLE products (id INT PRIMARY KEY, name VARCHAR) REPLICATE;
CREATE TABLE sales (id INT PRIMARY KEY, product_id INT) PARTITION BY id;

SELECT s.*, p.name
FROM sales s JOIN products p ON s.product_id = p.id;

-- products replicated to all partitions
-- Join executes locally (no network)
```

## Tags
`database-specific`, `voltdb`, `replication`, `local-join`, `dimension`
