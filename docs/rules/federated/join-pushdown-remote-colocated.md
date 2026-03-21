# Rule: "Join Pushdown: Co-located Tables"

**Category:** federated/pushdown
**File:** `rules/federated/join-pushdown-remote-colocated.rra`

## Metadata

- **ID:** `federated-join-pushdown-colocated`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, join, colocated
- **Authors:** "ra-optimizer"


# Join Pushdown: Co-located Tables

## Description

When both sides of a join reside on the same remote database, push
the entire join to that remote. This avoids all network transfer for
the join operation.

## Relational Algebra

```algebra
Join[cond](RemoteScan[t1, endpoint], RemoteScan[t2, endpoint])
=>
RemoteScan[pushdown_join(t1, t2, cond), endpoint]

Preconditions:
  - Both tables on same endpoint
  - Remote supports join pushdown
```

## Before

```
(Join :type INNER :condition (= orders.customer_id customers.id)
  :left (RemoteScan "orders" "db.example.com")
  :right (RemoteScan "customers" "db.example.com"))
```

## After

```
(RemoteScan "db.example.com"
  :pushdown_join [orders INNER JOIN customers
    ON orders.customer_id = customers.id])
```

## Test Cases

### Test 1: Co-located inner join

#### Input
```
(Join :type INNER :condition (= o.cid c.id)
  :left (RemoteScan "orders" "pg.example.com")
  :right (RemoteScan "customers" "pg.example.com"))
```

#### Expected
```
(RemoteScan "pg.example.com"
  :pushdown_join [orders INNER JOIN customers ON o.cid = c.id])
```
