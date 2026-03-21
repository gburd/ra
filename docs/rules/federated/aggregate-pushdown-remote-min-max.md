# Rule: "Aggregate Pushdown: MIN/MAX"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-min-max.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-min-max`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, min, max
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: MIN/MAX

## Description

Push MIN and MAX aggregations to the remote. Both are decomposable:
compute partial MIN/MAX remotely, take the global MIN/MAX locally.

## Relational Algebra

```algebra
Aggregate[MIN(col)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_agg=MIN(col)]

Aggregate[MAX(col)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_agg=MAX(col)]
```

## Before

```
(Aggregate :aggregates [(MIN price) (MAX price)]
  :input (RemoteScan "products" "db.example.com"))
```

## After

```
(RemoteScan "products" "db.example.com"
  :pushdown_agg [(MIN price) (MAX price)])
```

## Test Cases

### Test 1: Min and max pushdown

#### Input
```
(Aggregate :aggregates [(MIN price) (MAX price)]
  :input (RemoteScan "products" "pg.example.com"))
```

#### Expected
```
(RemoteScan "products" "pg.example.com"
  :pushdown_agg [(MIN price) (MAX price)])
```
