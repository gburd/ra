# Join Order Benchmark (JOB)

Implementation of the Join Order Benchmark on IMDB data.

## Overview

The Join Order Benchmark (JOB) is a comprehensive benchmark for evaluating query optimizer join ordering decisions. It consists of 113 queries on the Internet Movie Database (IMDB), featuring:

- **21 tables** with real-world relationships
- **5-15 table joins** per query
- **Complex predicates** with correlations
- **Data skew** and real-world distributions
- **Query templates** (1-33) with variants (a-f)

JOB is widely used by PostgreSQL, MySQL, Oracle, and SQL Server teams for optimizer validation.

## Dataset

- **Source**: May 2013 IMDB snapshot
- **Size**: ~1GB compressed, ~3GB uncompressed
- **Tables**: 21 (aka_name, aka_title, cast_info, char_name, comp_cast_type, company_name, company_type, complete_cast, info_type, keyword, kind_type, link_type, movie_companies, movie_info, movie_info_idx, movie_keyword, movie_link, name, person_info, role_type, title)
- **Rows**: ~60M total across all tables

## Setup

### 1. Download Dataset

```bash
./download_imdb.sh
```

This downloads the IMDB CSV files and query SQL files from the official JOB repository.

### 2. Load Data into PostgreSQL

```bash
./load_data.sh imdb
```

Creates the `imdb` database, loads the schema, imports CSV data, and runs ANALYZE.

### 3. Validate Data Integrity

```bash
./validate_data.sh imdb
```

Verifies row counts match expected values from the JOB paper.

## Running Benchmarks

### Optimizer Benchmarks (Ra Engine)

```bash
cargo bench --package ra-engine --bench job_benchmark
```

Measures Ra optimizer performance on all 113 JOB queries.

### Differential Testing (Ra vs PostgreSQL)

```bash
./run_job_comparison.sh imdb
```

Compares query execution and plans between Ra optimizer and PostgreSQL planner.

### Result Validation

```bash
./validate_results.sh imdb
```

Ensures Ra and PostgreSQL return identical results for all queries.

## Query Structure

JOB queries are organized into 33 templates with variants:

- **1a-1d**: Simple 3-4 table joins
- **2a-2d**: Medium complexity (5-7 tables)
- **3a-3c**: High complexity (8-10 tables)
- **4a-4c**: Very complex (11-15 tables)
- **...**
- **33a-33c**: Edge cases

Example query (13a.sql):
```sql
-- 7-way join with complex predicates
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

## Results

Performance comparison results are written to `results/job-ra-vs-pg.md`.

## Success Criteria

- ✅ **Correctness**: 100% of 113 queries return correct results
- 🎯 **Performance**: Ra matches or beats PostgreSQL on 80%+ of queries
- 📊 **Join Ordering**: Analysis of join order decisions (Ra vs PostgreSQL)
- ⏱️ **Optimization Time**: <5 seconds for complex queries (10+ tables)

## References

- [Original JOB Paper](https://db.in.tum.de/~leis/papers/jobench.pdf) - Leis et al., "How Good Are Query Optimizers, Really?"
- [JOB Repository](https://github.com/gregrahn/join-order-benchmark) - Official dataset and queries
- [IMDB Dataset](https://www.imdb.com/interfaces/) - Source data information
