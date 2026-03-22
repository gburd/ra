# Database Benchmark Research

## Executive Summary

**Recommended benchmarks for Ra optimizer validation:**
1. **Join Order Benchmark (JOB)** - Must have (113 queries, tests join optimization)
2. **TPC-DS** - High priority (99 queries, tests advanced SQL features)
3. **Star Schema Benchmark (SSB)** - Medium priority (13 queries, star schema patterns)
4. **HammerDB TPROC-C** - Medium priority (OLTP workload testing)

## HammerDB

**Overview**: Open-source database load testing tool implementing TPC-OSS benchmarks

- **Workloads**: TPROC-C (TPC-C-like OLTP), TPROC-H (TPC-H-like OLAP)
- **Databases**: PostgreSQL, Oracle, SQL Server, MySQL, MariaDB, Db2
- **Interfaces**: GUI, CLI, browser-based
- **Automation**: Official Docker images for CI/CD integration
- **Use Case**: Mission-critical performance testing, fair-use benchmarking
- **Website**: https://www.hammerdb.com/

### Integration Approach
```bash
# Docker-based HammerDB integration
docker pull hammerdb/hammerdb
docker run -it hammerdb/hammerdb hammerdbcli
# Configure for PostgreSQL + Ra optimizer
# Run TPROC-C or TPROC-H workloads
```

## TPC-H ✅

**Status**: Already implemented in Ra

- **Type**: Decision support (OLAP) benchmark
- **Queries**: 22 business-oriented queries
- **Focus**: Complex aggregations, joins, large scans
- **Dataset**: Scalable (SF1=1GB, SF10=10GB, SF100=100GB)
- **Complexity**: Moderate - tests optimizer basics

## Join Order Benchmark (JOB) ⭐⭐⭐⭐⭐

**Priority**: MUST HAVE - Highest value for testing join optimization

### Overview
- **Type**: Real-world join optimization benchmark
- **Paper**: "How Good Are Query Optimizers, Really?" (VLDB 2015)
- **Dataset**: IMDB (Internet Movie Database) - May 2013 snapshot
- **Schema**: 21 tables (movies, cast, companies, keywords, ratings, etc.)
- **Queries**: 113 queries total
  - 33 base query templates
  - Multiple variants (a-f suffixes) testing different join orders
- **Repository**: https://github.com/gregrahn/join-order-benchmark

### Why JOB is Critical

1. **Real Production Data**: Unlike synthetic TPC benchmarks, uses actual IMDB data with real correlations and skew
2. **Complex Joins**: Queries join 5-15 tables with intricate filter conditions
3. **Optimizer Killer**: Designed to expose weaknesses in query optimizers
4. **Join Order Testing**: Multiple query variants test if optimizer chooses optimal join order
5. **Industry Standard**: Used by PostgreSQL, MySQL, Oracle teams to validate optimizers

### Example Query Complexity
```sql
-- JOB Query 13a: 7-way join with complex predicates
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

### Implementation Plan

**Week 1: Dataset Setup**
- Download IMDB CSV files from CWI or IMDB interfaces
- Create 21-table schema in PostgreSQL
- Load data (compressed dataset ~1GB)
- Validate data integrity

**Week 2: Query Integration**
- Import all 113 SQL query files
- Create Ra benchmark harness
- Set up differential testing (Ra vs PostgreSQL)
- Implement result validation

**Deliverables**:
```
benchmarks/job/
├── README.md              # JOB documentation
├── schema.sql             # 21-table IMDB schema
├── data/                  # CSV files or download script
├── queries/               # 113 .sql files (1a.sql, 1b.sql, ...)
├── run_job_benchmark.sh   # Automated test script
└── results/
    ├── ra_results.json    # Ra execution times + plans
    └── pg_results.json    # PostgreSQL execution times + plans
```

### Success Metrics
- **Correctness**: 100% of queries return correct results
- **Performance**: Ra matches or beats PostgreSQL on 80%+ of queries
- **Join Orders**: Ra chooses optimal join order vs PostgreSQL's suboptimal choices
- **Regression Detection**: No query slower than PostgreSQL by >2x

## TPC-DS ⭐⭐⭐⭐

**Priority**: HIGH - Most comprehensive SQL feature coverage

### Overview
- **Type**: Advanced decision support benchmark
- **Queries**: 99 queries (vs 22 in TPC-H)
- **Schema**: 24 tables (vs 8 in TPC-H)
- **Dataset**: Scalable like TPC-H (1GB - 100TB)
- **Complexity**: VERY HIGH - most complex TPC benchmark
- **Website**: https://www.tpc.org/tpcds/

### Why TPC-DS Matters

1. **Advanced SQL Features**:
   - Window functions (RANK, ROW_NUMBER, LAG, LEAD)
   - Recursive CTEs (WITH RECURSIVE)
   - ROLLUP, CUBE, GROUPING SETS
   - Multiple correlated subqueries
   - Self-joins and complex outer joins
   - INTERSECT, EXCEPT set operations

2. **Real-World Complexity**: Queries model actual retail analytics scenarios with realistic complexity

3. **Industry Standard**: Used by major vendors (Oracle, SQL Server, Teradata) to demonstrate performance

4. **Ra Feature Testing**: Perfect for validating recently merged features:
   - CTEs ✅ (merged today)
   - Window functions ✅ (merged today)
   - Set operations ✅ (merged today)

### Example Query Complexity
```sql
-- TPC-DS Query 67: Window functions, CTEs, complex aggregation
WITH results AS (
  SELECT i_category, i_class, i_brand,
         SUM(ss_quantity) AS sum_sales,
         AVG(ss_quantity) AS avg_sales,
         RANK() OVER (PARTITION BY i_category
                      ORDER BY SUM(ss_quantity) DESC) AS rk
  FROM store_sales, date_dim, item
  WHERE ss_sold_date_sk = d_date_sk
    AND ss_item_sk = i_item_sk
    AND d_year = 2001
    AND d_moy = 11
  GROUP BY i_category, i_class, i_brand
)
SELECT * FROM results WHERE rk <= 100;
```

### Implementation Plan

**Week 1-2: Schema Setup**
- Implement 24-table TPC-DS schema
- Generate dataset using TPC-DS dbgen tool
- Load into PostgreSQL
- Validate cardinalities

**Week 3: Query Implementation**
- Import all 99 SQL query templates
- Parameterize queries for multiple runs
- Handle dialect differences (PostgreSQL syntax)

**Week 4: Integration**
- Create benchmark runner
- Implement result validation
- Set up automated testing

**Deliverables**:
```
benchmarks/tpcds/
├── README.md
├── schema.sql             # 24 tables
├── dbgen/                 # TPC-DS data generator
├── queries/               # 99 .sql files
├── run_tpcds_benchmark.sh
└── results/
```

### Success Metrics
- **Correctness**: All 99 queries execute successfully
- **Performance**: 70%+ of queries competitive with PostgreSQL
- **Feature Coverage**: Window functions, CTEs, set ops all validated
- **Optimizer Stress Test**: Handle complex multi-way joins

## Star Schema Benchmark (SSB) ⭐⭐⭐

**Priority**: MEDIUM - Good for specific optimization patterns

### Overview
- **Type**: Simplified OLAP benchmark
- **Queries**: 13 queries (4 query groups)
- **Schema**: Star schema - 1 fact table (lineorder), 4 dimension tables
- **Dataset**: Derived from TPC-H but denormalized
- **Focus**: Dimension filtering, star joins, aggregations
- **Paper**: "Star Schema Benchmark" (O'Neil et al.)

### Why SSB is Useful

1. **Star Schema Patterns**: Tests star join optimization (common in data warehouses)
2. **Dimension Filtering**: Tests predicate pushdown into dimension tables
3. **Denormalization**: Tests queries on denormalized schemas
4. **Simpler Than TPC-H**: Easier to implement, good baseline

### Query Structure
```sql
-- SSB Query 1.1: Simple dimension filter + aggregation
SELECT SUM(lo_extendedprice * lo_discount) AS revenue
FROM lineorder, date
WHERE lo_orderdate = d_datekey
  AND d_year = 1993
  AND lo_discount BETWEEN 1 AND 3
  AND lo_quantity < 25;
```

### Implementation Plan

**Week 1: Schema + Data**
- Create star schema (5 tables)
- Generate data from TPC-H or SSB generator
- Load into PostgreSQL

**Week 2: Queries + Testing**
- Implement 13 queries (4 groups)
- Set up benchmark harness
- Validate results

### Success Metrics
- **Correctness**: All 13 queries correct
- **Performance**: Match or beat PostgreSQL
- **Star Join Detection**: Ra recognizes star schema patterns

## HammerDB TPROC-C ⭐⭐⭐

**Priority**: MEDIUM - OLTP workload (different focus)

### Overview
- **Type**: OLTP (Online Transaction Processing) benchmark
- **Workload**: Based on TPC-C specification
- **Transactions**: 5 transaction types (New Order, Payment, Order Status, Delivery, Stock Level)
- **Focus**: High concurrency, short transactions, read-write mix

### Why TPROC-C Matters

1. **Transactional Workload**: Tests optimizer under OLTP conditions
2. **Different Patterns**: Short queries, point lookups, index usage
3. **Concurrency**: Tests optimizer under multi-user load
4. **Industry Standard**: TPC-C is the OLTP benchmark

### Caveat
TPROC-C is less relevant for Ra's query optimization focus (it's about transactions, not complex analytics), but useful for:
- Index selection validation
- Point query optimization
- Cost model calibration for OLTP

### Implementation Plan

**Week 1: HammerDB Setup**
- Docker-based HammerDB deployment
- Configure for PostgreSQL + Ra extension
- Set up TPROC-C schema (9 tables)

**Week 2: Workload Execution**
- Run TPROC-C workload (1-hour test)
- Collect metrics (TPM, response time)
- Compare Ra vs PostgreSQL optimizer

## Lower Priority Benchmarks

### LinkBench (⭐⭐)
- **Type**: Social graph workload (Facebook)
- **Use Case**: Graph traversals, range queries
- **Relevance**: Specialized, less relevant for general query optimization

### YCSB (⭐)
- **Type**: Key-value / NoSQL benchmark
- **Use Case**: Simple operations (insert, update, read)
- **Relevance**: Not relevant for relational query optimization

## Benchmark Implementation Priority

### Phase 1: Immediate (Q1 2026) ✅
- [x] TPC-H (already implemented)

### Phase 2: Next (Q2 2026) 🎯
1. **Join Order Benchmark (JOB)** - 2 weeks
   - Best ROI for join optimization testing
   - Real-world data exposes optimizer weaknesses
   - 113 queries provide comprehensive coverage

2. **TPC-DS** - 4 weeks
   - Validates all recent feature merges (CTEs, windows, set ops)
   - Industry standard for mature systems
   - 99 queries test advanced SQL

### Phase 3: Follow-up (Q3 2026)
3. **Star Schema Benchmark (SSB)** - 1 week
4. **HammerDB TPROC-C Integration** - 2 weeks

## Benchmark Test Harness Design

### Directory Structure
```
benchmarks/
├── README.md              # Overview of all benchmarks
├── runner/                # Shared benchmark infrastructure
│   ├── lib.rs            # Rust benchmark framework
│   ├── config.toml       # Benchmark configuration
│   └── differential.rs   # Ra vs PostgreSQL comparison
├── tpch/                 # ✅ Existing
│   ├── schema.sql
│   ├── queries/          # 22 queries
│   └── results/
├── job/                  # 🎯 Next priority
│   ├── README.md
│   ├── schema.sql        # 21 IMDB tables
│   ├── data/             # CSV files
│   ├── queries/          # 113 queries
│   └── results/
├── tpcds/                # 🎯 High priority
│   ├── schema.sql        # 24 tables
│   ├── queries/          # 99 queries
│   └── results/
├── ssb/                  # Later
│   └── ...
└── hammerdb/             # Later
    └── ...
```

### Benchmark Runner API
```rust
// benchmarks/runner/lib.rs

pub struct BenchmarkSuite {
    pub name: String,
    pub queries: Vec<BenchmarkQuery>,
    pub timeout: Duration,
}

pub struct BenchmarkQuery {
    pub id: String,
    pub sql: String,
    pub expected_rows: Option<usize>,
}

pub struct BenchmarkResults {
    pub suite: String,
    pub ra_results: Vec<QueryResult>,
    pub pg_results: Vec<QueryResult>,
    pub comparison: ComparisonReport,
}

pub struct QueryResult {
    pub query_id: String,
    pub execution_time_ms: f64,
    pub optimization_time_ms: f64,
    pub result_rows: usize,
    pub plan: String,
    pub success: bool,
    pub error: Option<String>,
}

pub fn run_benchmark_comparison(
    suite: &BenchmarkSuite,
    ra_optimizer: &Optimizer,
    pg_connection: &mut Client,
) -> Result<BenchmarkResults> {
    // 1. For each query in suite:
    //    - Run with Ra optimizer
    //    - Run with PostgreSQL planner
    //    - Compare results (correctness)
    //    - Compare performance
    // 2. Generate comparison report
    // 3. Export results (JSON, CSV, HTML report)
}

pub fn generate_report(results: &BenchmarkResults) -> String {
    // Generate HTML report with:
    // - Per-query comparison table
    // - Performance graphs
    // - Query plans
    // - Winner/loser analysis
}
```

### Key Metrics to Track

For each benchmark query:
1. **Correctness**: Result set matches (differential testing)
2. **Execution Time**: Ra vs PostgreSQL (ms)
3. **Optimization Time**: How long to generate plan (ms)
4. **Memory Usage**: Peak memory during execution
5. **Plan Quality**: Join order, operator selection
6. **Speedup Factor**: Ra time / PostgreSQL time

### Success Criteria

**Per-Benchmark Targets**:
- **TPC-H**: 90% of queries faster with Ra (already implemented)
- **JOB**: 80% of queries faster or equal (focus on join ordering)
- **TPC-DS**: 70% of queries competitive (harder due to complexity)
- **SSB**: 90% of queries faster (simpler star schema)

**Overall Goals**:
- **Correctness**: 100% across all benchmarks
- **No Regressions**: No query >2x slower than PostgreSQL
- **Demonstration**: Clear evidence Ra improves on PostgreSQL's optimizer

## Continuous Integration

### Automated Benchmark Runs
```yaml
# .github/workflows/benchmarks.yml
name: Benchmark Suite
on:
  push:
    branches: [main]
  schedule:
    - cron: '0 0 * * 0'  # Weekly

jobs:
  tpch:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Run TPC-H
        run: cargo bench --bench tpch
      - name: Store results
        uses: actions/upload-artifact@v3

  job:
    runs-on: ubuntu-latest
    steps:
      - name: Run Join Order Benchmark
        run: cargo bench --bench job
```

## Next Steps

1. **Immediate**: Implement JOB (2 weeks)
   - Highest value for join optimization validation
   - Real-world data exposes optimizer issues
   - Greg can start downloading IMDB dataset

2. **Q2 2026**: Implement TPC-DS (4 weeks)
   - Validates recent feature merges
   - Industry-standard advanced benchmark

3. **Ongoing**: Monitor benchmark results
   - Track performance over time
   - Detect regressions
   - Guide optimization priorities

## References

- **JOB Paper**: "How Good Are Query Optimizers, Really?" (Leis et al., VLDB 2015)
- **TPC-DS Spec**: https://www.tpc.org/tpcds/
- **HammerDB**: https://www.hammerdb.com/
- **SSB Paper**: "Star Schema Benchmark" (O'Neil et al.)
- **IMDB Dataset**: https://www.imdb.com/interfaces/
