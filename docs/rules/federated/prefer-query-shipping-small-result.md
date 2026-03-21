# Rule: "Prefer Query Shipping: Small Result Set"

**Category:** federated/strategy
**File:** `rules/federated/prefer-query-shipping-small-result.rra`

## Metadata

- **ID:** `federated-prefer-query-shipping-small-result`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, query-shipping, small-result
- **Authors:** "ra-optimizer"


# Prefer Query Shipping: Small Result Set

## Description

When the query result is expected to be small (relative to the input),
ship the entire query to the remote database. The remote executes the
full query and only the small result crosses the network.

## Decision Criteria

Ship query when:
- Remote supports all operators in the query
- Estimated result size < 10% of source data
- Result rows < configurable threshold (default: 100K rows)

## Relational Algebra

```algebra
IF estimated_result_rows(query) < threshold
   AND can_execute_remotely(query, capabilities)
THEN:
  query => ShipQuery(remote, query)
```

## Before

```
(Aggregate :aggregates [(COUNT *) (SUM amount)]
  :group_by [status]
  :input (RemoteScan "orders" "db.example.com"))
```

## After

```
(ShipQuery "db.example.com"
  (Aggregate :aggregates [(COUNT *) (SUM amount)]
    :group_by [status]
    :input (Scan "orders")))
```

## Cost Benefit

- Query shipping: latency(10ms) + remote_exec(50ms) + transfer(1KB) = ~60ms
- Data shipping: transfer(1GB at 100Mbps) + local_exec(500ms) = ~8500ms

## Test Cases

### Test 1: Aggregation produces small result

#### Input
```
(Aggregate :aggregates [(COUNT *)]
  :input (RemoteScan "events" "pg.example.com"))
```

#### Expected
Strategy: SHIP_QUERY
```
