# Rule: "Aggregate Pushdown: HAVING Clause"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-having.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-having`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, having, filter
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: HAVING Clause

## Description

Push GROUP BY with HAVING filter to the remote database. The HAVING
clause filters groups after aggregation, reducing the number of
groups transferred.

## Relational Algebra

```algebra
Filter[having_pred](Aggregate[GROUP BY keys, AGG](RemoteScan))
=>
RemoteScan[table, endpoint,
  pushdown_agg=[GROUP BY keys, AGG, HAVING having_pred]]
```

## Before

```
(Filter :predicate (> (SUM amount) 1000)
  :input (Aggregate :group_by [customer_id] :aggregates [(SUM amount)]
    :input (RemoteScan "orders" "db.example.com")))
```

## After

```
(RemoteScan "orders" "db.example.com"
  :pushdown_agg [GROUP BY customer_id, SUM(amount) HAVING SUM(amount) > 1000])
```

## Test Cases

### Test 1: Having clause pushdown

#### Input
```
(Filter :predicate (> total 1000)
  :input (Aggregate :group_by [customer_id] :aggregates [(SUM amount AS total)]
    :input (RemoteScan "orders" "pg.example.com")))
```

#### Expected
```
(RemoteScan "orders" "pg.example.com"
  :pushdown_agg [GROUP BY customer_id, SUM(amount) HAVING SUM(amount) > 1000])
```
