# Rule: "Prefer Data Shipping: Cross-Site Join with Small Side"

**Category:** federated/strategy
**File:** `rules/federated/prefer-data-shipping-cross-site-join.rra`

## Metadata

- **ID:** `federated-prefer-data-shipping-cross-site-join`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, strategy, data-shipping, cross-site-join
- **Authors:** "ra-optimizer"


# Prefer Data Shipping: Cross-Site Join with Small Side

## Description

When joining a local table with a remote table and the remote dataset
(after filtering) is small, fetch the remote data and join locally.
This avoids the complexity of distributed join strategies.

## Decision Criteria

Ship data when:
- One side of the join is local, the other remote
- Remote side (after filter pushdown) is small (< 100K rows)
- Local engine has sufficient memory for the join

## Relational Algebra

```algebra
IF small_side(join) = remote AND estimated_rows(remote) < threshold
THEN:
  Join[cond](LocalScan, RemoteScan) =>
    Join[cond](LocalScan, ShipData(remote, table, filter))
```

## Before

```
(Join :type INNER :condition (= l.id r.id)
  :left (Scan "local_table")
  :right (Filter :predicate (= status "ACTIVE")
    :input (RemoteScan "remote_table" "db.example.com")))
```

## After

```
(Join :type INNER :condition (= l.id r.id)
  :left (Scan "local_table")
  :right (ShipData "db.example.com" "remote_table"
    :pushdown_filter (= status "ACTIVE")))
```

## Test Cases

### Test 1: Cross-site join, fetch filtered remote

#### Input
```
(Join :type INNER :condition (= l.id r.id)
  :left (Scan "local_table")
  :right (Filter :predicate (= active true)
    :input (RemoteScan "remote_table" "pg.example.com")))
```

#### Expected
Strategy: SHIP_DATA with filter pushdown
```
