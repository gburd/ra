# JOB Benchmark Results: Ra vs PostgreSQL

> Template -- replace with actual results after running
> `./run_job_comparison.sh imdb`

Generated: YYYY-MM-DD HH:MM:SS UTC

## Executive Summary

Brief summary of overall results. How does Ra compare to PostgreSQL
across the 113 JOB queries? What is the geometric mean speedup?

- Total queries: 113
- Ra faster: ___ queries (___%)
- PostgreSQL faster: ___ queries (___%)
- Similar (within 10%): ___ queries (___%)
- Geometric mean speedup: ___x

## Query Performance Comparison

| Query | PG Time (ms) | Ra Time (ms) | Speedup | Join Width | Notes |
|-------|-------------|-------------|---------|-----------|-------|
| 1a    |             |             |         | 4         |       |
| 1b    |             |             |         | 4         |       |
| 1c    |             |             |         | 4         |       |
| 1d    |             |             |         | 4         |       |
| ...   |             |             |         |           |       |

## Join Ordering Analysis

Compare join trees produced by Ra vs PostgreSQL. Focus on queries where
the join order differs and the performance impact.

### Join Order Differences

| Query | PG Join Order       | Ra Join Order       | PG Cost  | Ra Cost  |
|-------|---------------------|---------------------|----------|----------|
|       |                     |                     |          |          |

### Observations

- Which join ordering strategies does Ra favor?
- Where does Ra's ordering diverge from PostgreSQL?
- How do cardinality estimation differences affect ordering?

## Optimization Time Breakdown

Time spent in the optimizer itself (not query execution).

| Join Width | Queries | Avg PG Plan (ms) | Avg Ra Plan (ms) |
|-----------|---------|------------------|-----------------|
| 3-5       |         |                  |                 |
| 6-8       |         |                  |                 |
| 9-11      |         |                  |                 |
| 12-15     |         |                  |                 |

Target: Ra optimization under 5 seconds for all queries.

## Queries Where Ra Excels

List the top 10 queries where Ra outperforms PostgreSQL, with
explanations of why.

### Query NN: ___x speedup

- **Root cause**: (e.g., better cardinality estimate for correlated
  columns)
- **PG plan**: (brief description)
- **Ra plan**: (brief description)
- **Key insight**: (what Ra got right)

## Queries Needing Improvement

List queries where PostgreSQL significantly outperforms Ra, with
analysis of what went wrong.

### Query NN: ___x slower

- **Root cause**: (e.g., cardinality overestimation on table X)
- **PG plan**: (brief description)
- **Ra plan**: (brief description)
- **Potential fix**: (what change to Ra would help)

## Follow-up RFCs Identified

Based on the benchmark results, these areas need RFC proposals:

- [ ] RFC NNNN: (title) -- addresses queries NN, NN, NN
- [ ] RFC NNNN: (title) -- addresses queries NN, NN

## Detailed Query Analysis

In-depth analysis of the 10 most interesting queries.

### Query NN

**Structure**: N-way join between tables A, B, C, ...

**PostgreSQL plan**:
```
(paste EXPLAIN ANALYZE output)
```

**Ra plan**:
```
(paste Ra optimizer output)
```

**Analysis**: Why does one optimizer outperform the other? What
cardinality estimates differ? What join methods were chosen?

---

### Query NN

(Repeat for each of the top 10 interesting queries.)
