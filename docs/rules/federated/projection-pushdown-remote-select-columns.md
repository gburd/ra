# Rule: "Projection Pushdown: Select Columns"

**Category:** federated/pushdown
**File:** `rules/federated/projection-pushdown-remote-select-columns.rra`

## Metadata

- **ID:** `federated-projection-pushdown-select-columns`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, projection, network-optimization
- **Authors:** "ra-optimizer"


# Projection Pushdown: Select Columns

## Description

Push column projections to the remote database to reduce the width of
transferred rows. Instead of SELECT *, only request the columns needed.

**When to apply**: When the query uses a subset of available columns
from a remote table.

**Why it works**: Narrower rows mean less data per row transferred
over the network.

## Relational Algebra

```algebra
Project[cols](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_project=cols]
```

## Before

```
(Project
  :columns [id, name]
  :input (RemoteScan "users" "db.example.com"))
```

## After

```
(RemoteScan "users" "db.example.com"
  :pushdown_project [id, name])
```

## Test Cases

### Test 1: Two column projection

#### Input
```
(Project :columns [id, name] :input (RemoteScan "users" "pg.example.com"))
```

#### Expected
```
(RemoteScan "users" "pg.example.com" :pushdown_project [id, name])
```
