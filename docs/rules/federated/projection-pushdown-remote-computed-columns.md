# Rule: "Projection Pushdown: Computed Columns"

**Category:** federated/pushdown
**File:** `rules/federated/projection-pushdown-remote-computed-columns.rra`

## Metadata

- **ID:** `federated-projection-pushdown-computed-columns`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, projection, computed-columns
- **Authors:** "ra-optimizer"


# Projection Pushdown: Computed Columns

## Description

Push computed column expressions to the remote database when the remote
supports the required functions. Reduces transfer by computing derived
values at the source.

**When to apply**: When projections include expressions using functions
supported by the remote database.

## Relational Algebra

```algebra
Project[computed_expr(col)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_project=computed_expr(col)]

Preconditions:
  - All functions in computed_expr are in remote.supported_functions
```

## Before

```
(Project
  :columns [(UPPER name), (CONCAT first_name " " last_name)]
  :input (RemoteScan "users" "db.example.com"))
```

## After

```
(RemoteScan "users" "db.example.com"
  :pushdown_project [(UPPER name), (CONCAT first_name " " last_name)])
```

## Test Cases

### Test 1: Upper function pushdown

#### Input
```
(Project :columns [(UPPER name)] :input (RemoteScan "users" "pg.example.com"))
```

#### Expected
```
(RemoteScan "users" "pg.example.com" :pushdown_project [(UPPER name)])
```
