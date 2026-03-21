# Rule: "Join Pushdown: Bind Join (Dependent Join)"

**Category:** federated/pushdown
**File:** `rules/federated/join-pushdown-remote-bind-join.rra`

## Metadata

- **ID:** `federated-join-pushdown-bind-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite
- **Tags:** federated, pushdown, join, bind-join, dependent-join
- **Authors:** "ra-optimizer"


# Join Pushdown: Bind Join (Dependent Join)

## Description

For each batch of rows from the local side, generate a parameterized
query against the remote with bound join keys. This avoids full table
transfers when the local side is small.

Also known as "dependent join" or "nested loop with pushdown".

## Relational Algebra

```algebra
Join[cond](LocalScan[t1], RemoteScan[t2, endpoint])
=>
BindJoin[batch_size=1000](
  LocalScan[t1],
  RemoteScan[t2, endpoint,
    parameterized_filter=cond])

Preconditions:
  - Local table is small (< 10K rows)
  - Remote supports parameterized queries
```

## Before

```
(Join :type INNER :condition (= local.key remote.key)
  :left (Scan "local_small")
  :right (RemoteScan "remote_table" "db.example.com"))
```

## After

```
(BindJoin :batch_size 1000
  :outer (Scan "local_small")
  :inner (RemoteScan "remote_table" "db.example.com"
    :parameterized_filter (= key ?bound_key)))
```

## Test Cases

### Test 1: Bind join with small local

#### Input
```
(Join :type INNER :condition (= l.key r.key)
  :left (Scan "local_100_rows")
  :right (RemoteScan "remote_table" "pg.example.com"))
```

#### Expected
```
(BindJoin :batch_size 100
  :outer (Scan "local_100_rows")
  :inner (RemoteScan "remote_table" "pg.example.com"
    :parameterized_filter (= key ?bound_key)))
```
