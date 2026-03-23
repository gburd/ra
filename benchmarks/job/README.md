# Join Order Benchmark (JOB)

Implementation of the Join Order Benchmark for evaluating Ra's query
optimizer against real-world workloads from the Internet Movie Database.

## Why JOB Matters

Most query optimizer benchmarks (TPC-H, TPC-DS) use synthetic data with
uniform distributions. JOB uses real IMDB data, which exposes optimizer
weaknesses that synthetic benchmarks miss:

- **Correlated columns**: Production year and genre are correlated in
  real movies. Optimizers that assume independence underestimate join
  cardinalities by 1000x or more.
- **Data skew**: A few actors appear in thousands of movies; most appear
  in one. Hash join vs. nested loop decisions depend on detecting this.
- **Multi-way joins**: 5-15 table joins per query mean the search space
  for join orderings is enormous (up to 15! permutations). Good
  heuristics and pruning are essential.
- **Real foreign keys**: IMDB has genuine referential relationships, not
  synthetic star/snowflake schemas. This tests whether the optimizer
  handles arbitrary join graphs.

The original paper (Leis et al., VLDB 2015) showed that all major
commercial optimizers produce suboptimal plans on JOB queries, often by
orders of magnitude. This makes JOB the standard benchmark for join
ordering research.

## Overview

| Property       | Value                                         |
|----------------|-----------------------------------------------|
| Queries        | 113 (33 templates, variants a-f)              |
| Tables         | 21                                            |
| Dataset        | IMDB May 2013 snapshot                        |
| Size           | ~1 GB compressed, ~3 GB on disk               |
| Total rows     | ~60M across all tables                        |
| Join width     | 5-15 tables per query                         |
| Join graph     | Arbitrary (not star/snowflake)                |

### Tables

aka_name, aka_title, cast_info, char_name, comp_cast_type, company_name,
company_type, complete_cast, info_type, keyword, kind_type, link_type,
movie_companies, movie_info, movie_info_idx, movie_keyword, movie_link,
name, person_info, role_type, title.

### Query Templates

Queries are organized into 33 templates with variants (a-f) that change
filter predicates while keeping the same join structure:

- **Templates 1-8**: 3-5 table joins (baseline difficulty)
- **Templates 9-16**: 5-8 table joins (moderate difficulty)
- **Templates 17-25**: 8-10 table joins (high difficulty)
- **Templates 26-33**: 10-15 table joins (stress tests)

Example query (13a.sql) -- a 7-way join with complex predicates:

```sql
SELECT MIN(cn.name) AS producing_company,
       MIN(miidx.info) AS rating,
       MIN(t.title) AS movie_title
FROM company_name cn,
     info_type it,
     kind_type kt,
     movie_companies mc,
     movie_info_idx miidx,
     movie_keyword mk,
     title t
WHERE kt.kind = 'movie'
  AND it.info = 'rating'
  AND cn.country_code = '[us]'
  AND cn.id = mc.company_id
  AND mc.movie_id = t.id
  AND t.id = miidx.movie_id
  AND miidx.info_type_id = it.id
  AND t.id = mk.movie_id
  AND kt.id = t.kind_id;
```

## Setup

### Prerequisites

- PostgreSQL 14+ with `psql`, `createdb`, `dropdb` on PATH
- ~4 GB free disk space
- Internet access for initial download

### Step 1: Download the IMDB Dataset

```bash
cd benchmarks/job
./download_imdb.sh
```

Clones the official JOB repository and copies CSV data files and SQL
query files into `data/` and `queries/` respectively.

### Step 2: Load Data into PostgreSQL

```bash
./load_data.sh imdb
```

Creates the `imdb` database, loads the schema (21 tables + indexes),
imports all CSV files via `\COPY`, and runs `ANALYZE` to populate
statistics.

### Step 3: Validate Data Integrity

```bash
./validate_data.sh imdb
```

Checks row counts against expected values from the JOB paper. All 21
tables must match. Minor differences may occur with different IMDB
snapshots.

### Step 4: Run PostgreSQL Baseline

```bash
./run_job_comparison.sh imdb
```

Executes all 113 queries against PostgreSQL and records execution times
and EXPLAIN plans. Results are written to `results/`.

### Step 5: Validate Result Correctness

```bash
./validate_results.sh imdb
```

Runs each query and verifies it returns non-empty results. When Ra
optimizer integration is complete, this will compare result sets between
Ra and PostgreSQL for 100% correctness.

## Usage Examples

### Run the full benchmark suite

```bash
# From repository root
cd benchmarks/job
./download_imdb.sh
./load_data.sh imdb
./validate_data.sh imdb
./run_job_comparison.sh imdb
```

### Run Ra optimizer benchmarks (Criterion)

```bash
cargo bench --package ra-engine --bench job_benchmark
```

Measures Ra optimizer performance on all 113 JOB queries using Criterion
for statistical rigor.

### Run a single query manually

```bash
psql -d imdb -f benchmarks/job/queries/13a.sql
```

### Get the PostgreSQL execution plan for a query

```bash
psql -d imdb -c "EXPLAIN (ANALYZE, FORMAT JSON) $(cat benchmarks/job/queries/13a.sql)"
```

## Interpreting Results

### Execution Time

The primary metric. Compare wall-clock time for each query between Ra
and PostgreSQL. A "speedup" greater than 1.0 means Ra is faster.

Key things to look for:

- **Queries where Ra is >2x faster**: Ra found a better join order.
  Examine the plan to understand why.
- **Queries where Ra is >2x slower**: Ra chose a poor join order.
  Investigate the cardinality estimates.
- **Queries with similar times**: The join ordering does not matter
  much for these queries (either because all orderings are similar,
  or the query is dominated by I/O).

### Join Order Quality

Compare the join trees produced by each optimizer. The JOB paper defines
"cost ratio" as (actual cost of chosen plan) / (cost of optimal plan).
A ratio of 1.0 is optimal; anything above 10 indicates a problem.

### Optimization Time

How long the optimizer itself takes (not query execution). For Ra, this
should be under 5 seconds even for 15-table joins.

### Cardinality Estimation

The root cause of most optimizer failures. Compare estimated
cardinalities in EXPLAIN output against actual row counts from
EXPLAIN ANALYZE. Large misestimations (>100x) typically lead to
poor join orderings.

## Success Criteria

- [x] **Correctness**: 100% of 113 queries return correct results
- [ ] **Performance**: Ra matches or beats PostgreSQL on 80%+ of queries
- [ ] **Join ordering**: Ra produces equal or better join orders
- [ ] **Optimization time**: Under 5 seconds for 10+ table joins

## File Layout

```
benchmarks/job/
  README.md                  # This file
  schema.sql                 # 21 IMDB tables + indexes
  download_imdb.sh           # Fetch dataset and queries
  load_data.sh               # Load into PostgreSQL
  validate_data.sh           # Check row counts
  run_job_comparison.sh      # Differential testing script
  validate_results.sh        # Result correctness validation
  queries/                   # 113 JOB SQL files (downloaded)
  data/                      # IMDB CSV files (downloaded)
  results/                   # Benchmark output
    job-ra-vs-pg.md          # Performance comparison template
    pg_plan_*.json           # PostgreSQL EXPLAIN output
```

## References

- Leis, V., Gubichev, A., Mirber, A., Boncz, P., Kemper, A., Neumann,
  T. "How Good Are Query Optimizers, Really?" PVLDB 9(3), 2015.
  <https://db.in.tum.de/~leis/papers/jobench.pdf>
- Official JOB repository:
  <https://github.com/gregrahn/join-order-benchmark>
- IMDB dataset interfaces:
  <https://www.imdb.com/interfaces/>
