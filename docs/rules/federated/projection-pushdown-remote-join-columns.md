# Rule: "Projection Pushdown: Join Key Columns Only"

**Category:** federated/pushdown
**File:** `rules/federated/projection-pushdown-remote-join-columns.rra`

## Metadata

- **ID:** `federated-projection-pushdown-join-columns`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, projection, join
- **Authors:** "ra-optimizer"


# Projection Pushdown: Join Key Columns Only

## Description

When a remote table participates in a join, push down a projection
that includes only the join key columns and any columns needed by
upper operators. Avoids transferring unused columns.

## Relational Algebra

```algebra
Join[cond uses key K](LocalScan, RemoteScan[table, endpoint])
=>
Join[cond uses key K](LocalScan,
  RemoteScan[table, endpoint, pushdown_project=K + needed_cols])
```

## Before

```
(Join :type INNER :condition (= local.id remote.id)
  :left (Scan "local_table")
  :right (RemoteScan "remote_table" "db.example.com"))
```

## After

```
(Join :type INNER :condition (= local.id remote.id)
  :left (Scan "local_table")
  :right (RemoteScan "remote_table" "db.example.com"
    :pushdown_project [id]))
```

## Test Cases

### Test 1: Join key only projection

#### Input
```
(Join :type INNER :condition (= local.id remote.id)
  :left (Scan "local")
  :right (RemoteScan "remote" "pg.example.com"))
```

#### Expected
```
(Join :type INNER :condition (= local.id remote.id)
  :left (Scan "local")
  :right (RemoteScan "remote" "pg.example.com"
    :pushdown_project [id]))
```
