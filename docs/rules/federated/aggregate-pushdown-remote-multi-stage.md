# Rule: "Aggregate Pushdown: Multi-Stage Aggregation"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-multi-stage.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-multi-stage`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, snowflake, bigquery, spark, duckdb
- **Tags:** federated, pushdown, aggregate, multi-stage, partial
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: Multi-Stage Aggregation

## Description

For queries aggregating data from multiple remote sources, use a
two-stage aggregation strategy: compute partial aggregates at each
remote, then merge locally.

This applies to decomposable aggregates: COUNT, SUM, MIN, MAX.
AVG is decomposed into SUM + COUNT.

## Relational Algebra

```algebra
Aggregate[AGG(col)](Union(RemoteScan[t1, ep1], RemoteScan[t2, ep2]))
=>
Aggregate[MERGE_AGG](Union(
  RemoteScan[t1, ep1, pushdown_agg=PARTIAL_AGG(col)],
  RemoteScan[t2, ep2, pushdown_agg=PARTIAL_AGG(col)]))
```

## Before

```
(Aggregate :aggregates [(SUM revenue)]
  :input (Union
    :left (RemoteScan "sales_us" "us.db.com")
    :right (RemoteScan "sales_eu" "eu.db.com")))
```

## After

```
(Aggregate :aggregates [(SUM partial_sum)]
  :input (Union
    :left (RemoteScan "sales_us" "us.db.com"
      :pushdown_agg [(SUM revenue AS partial_sum)])
    :right (RemoteScan "sales_eu" "eu.db.com"
      :pushdown_agg [(SUM revenue AS partial_sum)])))
```

## Test Cases

### Test 1: Two-stage sum across two remotes

#### Input
```
(Aggregate :aggregates [(SUM revenue)]
  :input (Union
    :left (RemoteScan "sales_us" "us.db.com")
    :right (RemoteScan "sales_eu" "eu.db.com")))
```

#### Expected
```
(Aggregate :aggregates [(SUM partial_sum)]
  :input (Union
    :left (RemoteScan "sales_us" "us.db.com" :pushdown_agg [(SUM revenue)])
    :right (RemoteScan "sales_eu" "eu.db.com" :pushdown_agg [(SUM revenue)])))
```
