# Rule: "Prefer Data Shipping: Small Remote Table"

**Category:** federated/strategy
**File:** `rules/federated/prefer-data-shipping-small-table.rra`

## Metadata

- **ID:** `federated-prefer-data-shipping-small-table`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, data-shipping, small-table
- **Authors:** "ra-optimizer"


# Prefer Data Shipping: Small Remote Table

## Description

When the remote table is small enough to transfer quickly, fetch all
of it locally. The overhead of query analysis and pushdown negotiation
may exceed the cost of just transferring the small dataset.

## Decision Criteria

Ship data when:
- Remote table < configurable threshold (default: 10MB)
- Transfer time < remote execution overhead
- Table fits in local memory

## Relational Algebra

```algebra
IF estimated_size(remote_table) < threshold
THEN:
  Op(RemoteScan[table, endpoint]) => Op(ShipData(endpoint, table))
```

## Before

```
(Filter :predicate (> score 90)
  :input (RemoteScan "config_values" "db.example.com"))
```

## After

```
(Filter :predicate (> score 90)
  :input (ShipData "db.example.com" "config_values"))
```

## Cost Benefit

- Small table (1000 rows, 200KB):
  - Ship data: latency(10ms) + transfer(2ms) + local_filter(1ms) = 13ms
  - Ship query: latency(10ms) + remote_exec(5ms) + transfer(0.5ms) = 15.5ms
  - Data shipping is simpler and comparable cost

## Test Cases

### Test 1: Small lookup table

#### Input
```
(Filter :predicate (= code "US")
  :input (RemoteScan "countries" "pg.example.com"))
```

#### Expected
Strategy: SHIP_DATA (table < 10MB threshold)
```
