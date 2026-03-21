# Rule: "Aggregate Pushdown: AVG via SUM/COUNT"

**Category:** federated/pushdown
**File:** `rules/federated/aggregate-pushdown-remote-avg.rra`

## Metadata

- **ID:** `federated-aggregate-pushdown-avg`
- **Version:** "1.0.0"
- **Databases:** postgresql, mysql, sqlite, snowflake, bigquery, duckdb
- **Tags:** federated, pushdown, aggregate, avg, decomposition
- **Authors:** "ra-optimizer"


# Aggregate Pushdown: AVG via SUM/COUNT Decomposition

## Description

AVG is not directly decomposable across sites, but it can be decomposed
into SUM and COUNT which are. Push SUM(col) and COUNT(col) to the remote,
then compute AVG = SUM/COUNT locally.

## Relational Algebra

```algebra
Aggregate[AVG(col)](RemoteScan[table, endpoint])
=>
Project[partial_sum / partial_count AS avg](
  RemoteScan[table, endpoint,
    pushdown_agg=[SUM(col) AS partial_sum, COUNT(col) AS partial_count]])
```

## Before

```
(Aggregate :aggregates [(AVG salary)]
  :input (RemoteScan "employees" "db.example.com"))
```

## After

```
(Project :columns [(/ partial_sum partial_count)]
  :input (RemoteScan "employees" "db.example.com"
    :pushdown_agg [(SUM salary AS partial_sum)
                   (COUNT salary AS partial_count)]))
```

## Test Cases

### Test 1: AVG decomposition

#### Input
```
(Aggregate :aggregates [(AVG salary)]
  :input (RemoteScan "employees" "pg.example.com"))
```

#### Expected
```
(Project :columns [(/ partial_sum partial_count)]
  :input (RemoteScan "employees" "pg.example.com"
    :pushdown_agg [(SUM salary) (COUNT salary)]))
```
