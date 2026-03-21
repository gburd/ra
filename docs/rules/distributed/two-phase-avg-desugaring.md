# Rule: Two-Phase AVG Desugaring

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/two-phase-avg-desugaring.rra`

## Metadata

- **ID:** `two-phase-avg-desugaring`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, aggregation, two-phase, avg, desugaring
- **Authors:** "RA Contributors"


# Two-Phase AVG Desugaring

## Description

AVG is not directly decomposable (average of averages != average of all).
Desugar AVG(x) into SUM(x)/COUNT(x), where both SUM and COUNT are
individually decomposable. Local phase computes partial SUM and COUNT,
global phase sums the partials and divides.

**When to apply**: Any AVG aggregate on distributed data.

## Relational Algebra

```algebra
-- Before
gamma[g, AVG(x)](R)

-- After
pi[g, sum_x / count_x AS avg_x](
    gamma[g, SUM(partial_sum) AS sum_x, SUM(partial_count) AS count_x](
        Exchange[hash(g)](
            gamma[g, SUM(x) AS partial_sum, COUNT(x) AS partial_count](R)
        )
    )
)
```

## Test Cases

```sql
-- Positive: basic AVG
SELECT department, AVG(salary) FROM employees GROUP BY department;
-- Desugars to SUM(salary)/COUNT(salary)

-- Positive: AVG with other aggregates
SELECT region, AVG(price), SUM(qty) FROM sales GROUP BY region;
-- AVG desugared, SUM handled normally
```
