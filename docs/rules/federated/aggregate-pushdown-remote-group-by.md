# Rule: "Aggregate Pushdown: GROUP BY"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-group-by.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-group-by`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, group-by
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: GROUP BY

## Description

Push GROUP BY with decomposable aggregates to the remote database.
The remote produces partial aggregates per group, transferred back
for local merging.

## Relational Algebra

```algebra
Aggregate[GROUP BY keys, AGG(col)](RemoteScan[table, endpoint])
=>
Aggregate[GROUP BY keys, MERGE_AGG](
  RemoteScan[table, endpoint,
    pushdown_agg=[GROUP BY keys, PARTIAL_AGG(col)]])
```

## Before

```
(Aggregate :group_by [region] :aggregates [(SUM sales)]
  :input (RemoteScan "transactions" "db.example.com"))
```

## After

```
(RemoteScan "transactions" "db.example.com"
  :pushdown_agg [GROUP BY region, SUM(sales)])
```

## Test Cases

### Test 1: Group by with sum

#### Input
```
(Aggregate :group_by [region] :aggregates [(SUM sales)]
  :input (RemoteScan "transactions" "pg.example.com"))
```

#### Expected
```
(RemoteScan "transactions" "pg.example.com"
  :pushdown_agg [GROUP BY region, SUM(sales)])
```
