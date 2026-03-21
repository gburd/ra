# Rule: "Aggregate Pushdown: COUNT DISTINCT"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-distinct-count.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-distinct-count`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, count-distinct
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: COUNT DISTINCT

## Description

Push COUNT(DISTINCT col) to the remote when supported. COUNT DISTINCT
is not decomposable in general, but can be pushed when the entire
aggregation runs on one remote.

For multi-site COUNT DISTINCT, use approximate methods like HyperLogLog.

## Relational Algebra

```algebra
Aggregate[COUNT(DISTINCT col)](RemoteScan[table, endpoint])
=>
RemoteScan[table, endpoint, pushdown_agg=COUNT(DISTINCT col)]

Preconditions:
  - Single remote source (not cross-site)
  - OR remote supports APPROX_COUNT_DISTINCT
```

## Before

```
(Aggregate :aggregates [(COUNT DISTINCT user_id)]
  :input (RemoteScan "events" "db.example.com"))
```

## After

```
(RemoteScan "events" "db.example.com"
  :pushdown_agg [(COUNT DISTINCT user_id)])
```

## Test Cases

### Test 1: Count distinct pushdown

#### Input
```
(Aggregate :aggregates [(COUNT DISTINCT user_id)]
  :input (RemoteScan "events" "pg.example.com"))
```

#### Expected
```
(RemoteScan "events" "pg.example.com"
  :pushdown_agg [(COUNT DISTINCT user_id)])
```
