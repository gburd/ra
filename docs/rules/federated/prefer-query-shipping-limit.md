# Rule: "Prefer Query Shipping: LIMIT Clause"

**Category:** federated/strategy
**File:** `rules/federated/prefer-query-shipping-limit.rra`

## Metadata

- **ID:** `federated-prefer-query-shipping-limit`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, query-shipping, limit
- **Authors:** "ra-optimizer"


# Prefer Query Shipping: LIMIT Clause

## Description

When the query includes a LIMIT clause, ship the entire query to the
remote. LIMIT bounds the result size, making query shipping always
efficient regardless of the source table size.

## Decision Criteria

Ship query when:
- Query has LIMIT N where N is small (< 10K)
- Remote supports LIMIT pushdown
- Remote can execute the full query

## Relational Algebra

```algebra
IF has_limit(query) AND limit_value < threshold
   AND can_execute_remotely(query)
THEN:
  Limit[N](RemoteScan) => ShipQuery(remote, Limit[N](Scan))
```

## Before

```
(Limit :count 10 :offset 0
  :input (Sort :keys [(created_at DESC)]
    :input (RemoteScan "events" "db.example.com")))
```

## After

```
(ShipQuery "db.example.com"
  (Limit :count 10 :offset 0
    :input (Sort :keys [(created_at DESC)]
      :input (Scan "events"))))
```

## Test Cases

### Test 1: Top-N query

#### Input
```
(Limit :count 10
  :input (Sort :keys [(score DESC)]
    :input (RemoteScan "leaderboard" "pg.example.com")))
```

#### Expected
Strategy: SHIP_QUERY
```
