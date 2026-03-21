# Rule: "Projection Pushdown: Combined with Filter"

**Category:** federated/pushdown
**File:** `rules/federated/projection-pushdown-remote-with-filter.rra`

## Metadata

- **ID:** `federated-projection-pushdown-with-filter`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, projection, filter, combined
- **Authors:** "ra-optimizer"


# Projection Pushdown: Combined with Filter

## Description

Push both projection and filter to the remote database simultaneously.
This combines row-level and column-level reduction for maximum network
savings.

## Relational Algebra

```algebra
Project[cols](Filter[pred](RemoteScan[table, endpoint]))
=>
RemoteScan[table, endpoint, pushdown_filter=pred, pushdown_project=cols]
```

## Before

```
(Project :columns [id, name]
  :input (Filter :predicate (= active true)
    :input (RemoteScan "users" "db.example.com")))
```

## After

```
(RemoteScan "users" "db.example.com"
  :pushdown_filter (= active true)
  :pushdown_project [id, name])
```

## Test Cases

### Test 1: Filter and project combined

#### Input
```
(Project :columns [id, name]
  :input (Filter :predicate (= active true)
    :input (RemoteScan "users" "pg.example.com")))
```

#### Expected
```
(RemoteScan "users" "pg.example.com"
  :pushdown_filter (= active true)
  :pushdown_project [id, name])
```
