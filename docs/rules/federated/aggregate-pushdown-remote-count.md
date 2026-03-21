# Rule: "Aggregate Pushdown: COUNT"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-count.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-count`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, count
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: COUNT

## Description

Push COUNT aggregation to the remote database. Instead of transferring
all rows and counting locally, let the remote compute the count and
transfer a single value.

## Relational Algebra

```algebra
Aggregate[COUNT(*)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_agg=COUNT(*)]
```

## Before

```
(Aggregate :aggregates [(COUNT *)]
  :input (RemoteScan "orders" "db.example.com"))
```

## After

```
(RemoteScan "orders" "db.example.com"
  :pushdown_agg [(COUNT *)])
```

## Cost Benefit

- Without pushdown: Transfer 10M rows, count locally
- With pushdown: Transfer 1 row (the count) - 10,000,000x reduction

## Test Cases

### Test 1: Simple count

#### Input
```
(Aggregate :aggregates [(COUNT *)] :input (RemoteScan "orders" "pg.example.com"))
```

#### Expected
```
(RemoteScan "orders" "pg.example.com" :pushdown_agg [(COUNT *)])
```
