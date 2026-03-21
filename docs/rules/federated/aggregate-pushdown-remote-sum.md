# Rule: "Aggregate Pushdown: SUM"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-sum.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-sum`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, sum
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: SUM

## Description

Push SUM aggregation to the remote database. SUM is decomposable:
partial sums can be computed remotely and combined locally.

## Relational Algebra

```algebra
Aggregate[SUM(col)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_agg=SUM(col)]
```

## Before

```
(Aggregate :aggregates [(SUM amount)]
  :input (RemoteScan "orders" "db.example.com"))
```

## After

```
(RemoteScan "orders" "db.example.com"
  :pushdown_agg [(SUM amount)])
```

## Test Cases

### Test 1: Simple sum

#### Input
```
(Aggregate :aggregates [(SUM amount)] :input (RemoteScan "orders" "pg.example.com"))
```

#### Expected
```
(RemoteScan "orders" "pg.example.com" :pushdown_agg [(SUM amount)])
```
