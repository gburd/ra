# Rule: Distributed Exact Distinct Count

**Category:** distributed/aggregation
**File:** `rules/distributed/aggregation/distributed-distinct-count-exact.rra`

## Metadata

- **ID:** `distributed-distinct-count-exact`
- **Version:** "1.0.0"
- **Databases:** presto, trino, spark, cockroachdb, greenplum
- **Tags:** distributed, aggregation, distinct, exact, three-phase
- **Authors:** "RA Contributors"


# Distributed Exact Distinct Count

## Description

Exact COUNT(DISTINCT x) in a distributed environment using three-phase
aggregation: (1) local dedup to remove per-node duplicates, (2) shuffle
by distinct column to co-locate remaining duplicates, (3) final count
of globally-unique values.

**When to apply**: Exact distinct count required and approximate methods
(HLL) are not acceptable. Column has significant local duplication
(NDV < rows_per_node * 0.5).

## Relational Algebra

```algebra
-- Phase 1: Local dedup
-- Phase 2: Shuffle by distinct key
-- Phase 3: Final count
gamma[g, COUNT(*)](
    Exchange[hash(g)](
        delta[g, d](
            Exchange[hash(d)](
                delta[g, d](R)  -- local dedup
            )
        )
    )
)
```

## Test Cases

```sql
-- Positive: exact distinct count with duplication
SELECT department, COUNT(DISTINCT employee_id)
FROM timesheets GROUP BY department;
-- Each employee has many timesheet entries

-- Negative: low duplication (primary key)
SELECT dept, COUNT(DISTINCT timesheet_id) FROM timesheets GROUP BY dept;
-- timesheet_id is unique, no benefit from local dedup
```
