# Rule: "Projection Pushdown: Star Elimination"

**Category:** federated/pushdown
**File:** `rules/federated/projection-pushdown-remote-star-elimination.rra`

## Metadata

- **ID:** `federated-projection-pushdown-star-elimination`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, projection, star-elimination
- **Authors:** "ra-optimizer"


# Projection Pushdown: Star Elimination

## Description

Replace SELECT * on remote tables with explicit column lists based on
upstream consumption. Only request columns that are actually used by
operators above the remote scan.

**When to apply**: When SELECT * is used on a wide remote table but
only a few columns are referenced by parent operators.

## Relational Algebra

```algebra
Op[uses cols C](RemoteScan[table, endpoint, SELECT *])
=>
Op[uses cols C](RemoteScan[table, endpoint, pushdown_project=C])
```

## Before

```
(Filter :predicate (= id 42)
  :input (RemoteScan "wide_table" "db.example.com"))
```

## After

```
(Filter :predicate (= id 42)
  :input (RemoteScan "wide_table" "db.example.com"
    :pushdown_project [id]))
```

## Test Cases

### Test 1: Narrow from wide table

#### Input
```
(Filter :predicate (= id 42)
  :input (RemoteScan "wide_table" "pg.example.com"))
```

#### Expected
```
(Filter :predicate (= id 42)
  :input (RemoteScan "wide_table" "pg.example.com"
    :pushdown_project [id]))
```
