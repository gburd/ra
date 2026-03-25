# Benchmarks

Ra includes benchmark suites for measuring optimizer performance across
standard database workloads. These benchmarks measure **optimization
time** (how long Ra takes to find an optimal plan), not query execution
time.

## Benchmark Suites

### Join Order Benchmark (JOB)

The JOB benchmark uses all 113 queries from the IMDB dataset, as
defined in "How Good Are Query Optimizers, Really?" (Leis et al.).
Queries range from 2-table joins to 17-table joins, exercising Ra's
join reordering, predicate pushdown, and cost-based optimization.

**Dataset:** 21 IMDB tables (cast_info: 36M rows, movie_info: 14.8M
rows, title: 2.5M rows, etc.)

```bash
# Run the JOB benchmark
cargo bench --package ra-engine --bench job_benchmark
```

### TPC-H Benchmark (22 queries)

All 22 TPC-H queries at Scale Factor 1, covering aggregation, joins,
subqueries, set operations, and correlated predicates.

**Dataset statistics (SF=1):** lineitem: 6M rows, orders: 1.5M rows,
customer: 150K rows, supplier: 10K rows, part: 200K rows, partsupp:
800K rows, nation: 25 rows, region: 5 rows.

```bash
# Run the TPC-H benchmark
cargo bench --package ra-engine --bench tpch_all22
```

#### TPC-H Optimization Times

| Query | Description | Tables | Joins | Ra Time (ms) | Category |
|-------|-------------|--------|-------|-------------|----------|
| Q01 | Pricing summary report | 1 | 0 | 41 | Simple |
| Q02 | Minimum cost supplier | 5 | 4 | 1,286 | Complex |
| Q03 | Shipping priority | 3 | 2 | 1,147 | Medium |
| Q04 | Order priority checking | 2 | 1 | 41 | Simple |
| Q05 | Local supplier volume | 6 | 5 | 1,409 | Complex |
| Q06 | Forecasting revenue change | 1 | 0 | 50 | Simple |
| Q07 | Volume shipping | 6 | 5 | 1,445 | Complex |
| Q08 | National market share | 8 | 7 | 411 | Complex |
| Q09 | Product type profit | 6 | 5 | 1,305 | Complex |
| Q10 | Returned item reporting | 4 | 3 | 1,412 | Complex |
| Q11 | Important stock | 3 | 2 | 930 | Medium |
| Q12 | Shipping modes | 2 | 1 | 954 | Medium |
| Q13 | Customer distribution | 2 | 1 | 43 | Simple |
| Q14 | Promotion effect | 2 | 1 | 800 | Medium |
| Q15 | Top supplier | 2 | 1 | 2,755 | Complex |
| Q16 | Parts/supplier relationship | 3 | 2 | 1,096 | Medium |
| Q17 | Small-quantity-order revenue | 2 | 1 | 741 | Medium |
| Q18 | Large volume customer | 3 | 2 | 782 | Medium |
| Q19 | Discounted revenue | 2 | 1 | 677 | Medium |
| Q20 | Potential part promotion | 4 | 3 | 1,906 | Complex |
| Q21 | Suppliers kept orders waiting | 5+ | 4+ | 1,479 | Complex |
| Q22 | Global sales opportunity | 2 | 1 | 96 | Simple |

#### Summary by Category

| Category | Queries | Mean (ms) | Median (ms) | Range (ms) |
|----------|---------|-----------|-------------|------------|
| Simple (0-1 joins) | 5 | 54 | 43 | 41--96 |
| Medium (1-2 joins) | 8 | 891 | 867 | 677--1,147 |
| Complex (3+ joins) | 9 | 1,490 | 1,412 | 411--2,755 |
| **All 22 queries** | **22** | **868** | **867** | **41--2,755** |

### Plan Cache Benchmark

Simulates OLTP workloads with repeated query templates to measure
plan cache effectiveness.

```bash
# Run the plan cache benchmark
cargo bench --package ra-engine --bench plan_cache_bench
```

#### Plan Cache Results

| Configuration | Time (200 queries) | Speedup |
|--------------|-------------------|---------|
| No cache | 64.85 ms | 1.0x (baseline) |
| With cache | 1.75 ms | **37x** |

**Cached lookup cost:** 0.46 us per query (706x faster than full
optimization).

| Templates | Time per 200 queries | Hit Rate |
|-----------|---------------------|----------|
| 1 template | 590 us | 99.5% |
| 3 templates | 1.15 ms | 98.5% |
| 5 templates | 1.69 ms | 97.5% |

### Rule Priority Benchmark

Measures the impact of RFC 0058 rule complexity prioritization on
optimization time for multi-table join queries.

```bash
# Run the rule priority benchmark
cargo bench --package ra-engine --bench rule_priority_bench
```

Expected improvement: 20-27% faster optimization on complex queries
without sacrificing solution quality, by applying high-benefit,
low-complexity rules first.

## Ra vs PostgreSQL Planner

Ra uses equality saturation (exhaustive search) while PostgreSQL uses
heuristic top-down planning with GEQO for large join graphs. The
tradeoff: Ra spends more time planning but produces provably optimal
plans within its rule set.

| Metric | Ra Optimizer | PostgreSQL Planner |
|--------|-------------|-------------------|
| Approach | Equality saturation | Heuristic + GEQO |
| Simple queries | 41-96ms | 0.1-2ms |
| Medium queries | 677-1,147ms | 1-10ms |
| Complex queries | 411-2,755ms | 5-50ms |
| Optimality | Provably optimal within rule set | Heuristic |

### Where Ra Produces Better Plans

| Optimization | Ra | PostgreSQL | TPC-H Queries |
|-------------|-----|-----------|---------------|
| Global join reordering | All orderings explored | Greedy/GEQO for >12 tables | Q5, Q7, Q8, Q9, Q10 |
| Cross-operator optimization | Rules see full plan | Fixed-order passes | Q15, Q18 |
| Bidirectional predicate pushdown | Yes | Top-down only | Q3, Q5, Q7, Q10 |
| Aggregate pushdown through joins | Yes | Limited (PG15+) | Q5, Q7, Q8 |
| Semi/anti-join recognition | Pattern matching | Subquery flattening | Q4, Q16, Q20, Q21 |
| Runtime filter candidates | Bloom filter placement | Not in standard PG | Q5, Q8, Q20 |

## Additional Benchmarks

```bash
# Distributed TPC-H (multi-node simulation)
cargo bench --package ra-engine --bench tpch_distributed

# Resource budget enforcement
cargo bench --package ra-engine --bench resource_budgets

# Subquery unnesting
cargo bench --package ra-engine --bench unnest_bench

# Hardware cost models
cargo bench --package ra-hardware --bench hardware_models

# Dialect translation
cargo bench --package ra-dialect --bench backend_comparison

# Streaming statistics
cargo bench --package ra-stats --bench streaming_bench
```

## Running All Benchmarks

```bash
# Run all benchmarks across all crates
cargo bench

# Run benchmarks for a specific crate
cargo bench --package ra-engine

# Run a specific benchmark with output
cargo bench --package ra-engine --bench job_benchmark -- --verbose
```

Benchmark results are stored in `target/criterion/` with HTML reports
for trend analysis across commits.

## Setting Up the JOB Benchmark Data

The JOB benchmark uses IMDB data. To download and prepare the dataset:

```bash
cd benchmarks/job

# Download IMDB data files
./download_imdb.sh

# Load into a local database (optional, for comparison)
./load_data.sh

# Validate data integrity
./validate_data.sh
```

See `benchmarks/job/README.md` for details on the IMDB dataset and
query definitions.
