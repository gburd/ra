# Rule: "Prefer Data Shipping: Complex Unsupported Query"

**Category:** federated/strategy
**File:** `rules/federated/prefer-data-shipping-complex-query.rra`

## Metadata

- **ID:** `federated-prefer-data-shipping-complex-query`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, data-shipping, complex-query
- **Authors:** "ra-optimizer"


# Prefer Data Shipping: Complex Unsupported Query

## Description

When the query uses operations not supported by the remote (window
functions, recursive CTEs, complex subqueries), fetch the data and
execute locally. This is the fallback strategy.

## Decision Criteria

Ship data when:
- Query contains operations remote cannot handle
- Cannot decompose query into pushable and local parts
- Remote has limited capabilities (e.g., GenericJDBC)

## Relational Algebra

```algebra
IF NOT can_execute_remotely(query)
   AND NOT can_plan_hybrid(query)
THEN:
  ComplexOp(RemoteScan) => ComplexOp(ShipData(remote, table))
```

## Before

```
(Window :functions [(ROW_NUMBER OVER (PARTITION BY dept ORDER BY salary DESC))]
  :input (RemoteScan "employees" "legacy.jdbc.com"))
```

## After

```
(Window :functions [(ROW_NUMBER OVER (PARTITION BY dept ORDER BY salary DESC))]
  :input (ShipData "legacy.jdbc.com" "employees"))
```

## Test Cases

### Test 1: Window function unsupported remotely

#### Input
```
(Window :functions [(ROW_NUMBER)]
  :input (RemoteScan "employees" "legacy.jdbc.com"))
```

#### Expected
Strategy: SHIP_DATA (remote does not support window functions)
```
