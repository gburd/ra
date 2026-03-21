# Rule: "Prefer Query Shipping: High Selectivity Filter"

**Category:** federated/strategy
**File:** `rules/federated/prefer-query-shipping-high-selectivity.rra`

## Metadata

- **ID:** `federated-prefer-query-shipping-high-selectivity`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, query-shipping, selectivity
- **Authors:** "ra-optimizer"


# Prefer Query Shipping: High Selectivity Filter

## Description

When a query has a highly selective filter (selectivity < 1%) and the
remote can execute it, ship the query. The result will be tiny compared
to the full table.

## Decision Criteria

Ship query when:
- Filter selectivity < 0.01
- Remote supports filter pushdown
- No local-only operations required

## Relational Algebra

```algebra
IF selectivity(filter) < 0.01
   AND can_push_filter(remote)
THEN:
  Filter[pred](RemoteScan) => ShipQuery(remote, Filter[pred](Scan))
```

## Before

```
(Filter :predicate (= id 12345)
  :input (RemoteScan "users" "db.example.com"))
```

## After

```
(ShipQuery "db.example.com"
  (Filter :predicate (= id 12345)
    :input (Scan "users")))
```

## Test Cases

### Test 1: Point lookup by primary key

#### Input
```
(Filter :predicate (= id 42)
  :input (RemoteScan "users" "pg.example.com"))
```

#### Expected
Strategy: SHIP_QUERY
```
