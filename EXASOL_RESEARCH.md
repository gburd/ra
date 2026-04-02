# Exasol RDBMS - Research for Ra Optimizer Rules

**Date:** 2026-04-02
**Purpose:** Add Exasol-specific optimization rules to Ra
**TPC-H:** Winner for 100GB-100TB range
**Architecture:** In-memory, clustered OLAP database

---

## Executive Summary

**Exasol** is an in-memory, massively parallel processing (MPP) analytic database designed for OLAP workloads. Key differentiators:

1. **In-Memory Columnar Storage** - All data in RAM for sub-second queries
2. **Cluster Architecture** - Scales to hundreds of nodes
3. **MDX Support** - Multidimensional Expressions for OLAP cubes
4. **TPC-H Leadership** - Best performance in 100GB-100TB range
5. **Proprietary Optimizations** - Advanced query optimization techniques

---

## Key Features to Model in Ra

### 1. In-Memory Columnar Storage

**Architecture:**
- Columnar storage format (like DuckDB, but in-memory)
- Compression optimized for in-memory access
- SIMD vectorization for column scans
- Late materialization (process column indexes, materialize results last)

**Optimization Opportunities:**
```
Scan(table, columns=[a, b, c])
→ ColumnarScan(table, columns=[a, b, c], in_memory=true)

# Pushdown filters to column level
Filter(ColumnarScan(...), a > 10)
→ ColumnarScan(..., filter=[a > 10], bloom_filter=true)

# Late materialization
Project(Filter(Scan(t), cond), [a, b])
→ LateMaterialize(
    FilterColumnIndex(ScanColumnIndex(t.a), cond),
    [a, b]
  )
```

**Rules to Add:**
1. `columnar-scan-conversion` - Convert scans to columnar when in-memory
2. `late-materialization` - Defer tuple reconstruction
3. `column-predicate-pushdown` - Push filters to column indexes
4. `bloom-filter-generation` - Generate bloom filters for joins

---

### 2. Cluster Parallelization

**Architecture:**
- MPP: Data distributed across nodes via hash/range partitioning
- Parallel execution: All nodes execute simultaneously
- Smart shuffle: Minimize data movement between nodes
- Adaptive parallelism: Dynamic task scheduling based on cluster load

**Optimization Opportunities:**
```
# Distributed joins
Join(ScanA, ScanB, key)
→ DistributedJoin(
    ScanA(partition_key=key, nodes=N),
    ScanB(partition_key=key, nodes=N),
    key,
    strategy=colocated
  )

# Parallel aggregation
Agg(Scan(t), group=[a], agg=[sum(b)])
→ ParallelAgg(
    LocalAgg(ScanPartition(t, nodes=N), [a], [sum(b)]),
    FinalAgg([a], [sum(sum_b)])
  )

# Broadcast vs shuffle decision
Join(small_table, large_table, key)
→ BroadcastJoin(small_table, large_table, key)
  if size(small_table) < broadcast_threshold

Join(table1, table2, key)
→ ShuffleJoin(table1, table2, key, partition_by=key)
  if size(table1) ≈ size(table2)
```

**Rules to Add:**
1. `distributed-join-planning` - Choose join strategy based on data size
2. `parallel-aggregation` - Two-phase aggregation (local + global)
3. `broadcast-vs-shuffle` - Cost-based broadcast threshold
4. `colocated-join-detection` - Detect when data is already colocated
5. `partition-pruning` - Eliminate partitions at planning time

---

### 3. MDX (Multidimensional Expressions)

**What is MDX:**
- SQL extension for OLAP cubes
- Navigate dimensions (time, geography, product)
- Hierarchical queries (drill-down, roll-up)
- Calculated members and measures

**Example MDX Query:**
```mdx
SELECT
  { [Measures].[Sales], [Measures].[Quantity] } ON COLUMNS,
  NON EMPTY [Product].[Category].Members ON ROWS
FROM [Sales_Cube]
WHERE [Time].[2023]
```

**Equivalent SQL:**
```sql
SELECT
  p.category,
  SUM(s.sales) as sales,
  SUM(s.quantity) as quantity
FROM sales s
JOIN product p ON s.product_id = p.product_id
WHERE s.year = 2023
GROUP BY p.category
HAVING SUM(s.sales) > 0
```

**Optimization Opportunities:**
```
# MDX dimension hierarchy
MDX_Query(
  measures=[Sales, Quantity],
  dimensions=[Product.Category],
  filter=[Time.2023]
)
→ SQL_Query(
    SELECT p.category, SUM(sales), SUM(quantity)
    FROM sales JOIN product ON ...
    WHERE year = 2023
    GROUP BY p.category
  )

# Hierarchy roll-up
MDX_RollUp([Product].[Category].[Subcategory], level=Category)
→ GroupBy(
    Project(Scan(product), [category, subcategory]),
    [category]
  )

# Time series optimization
MDX_TimeRange([Time].[Year].[2020]:[2023])
→ Filter(Scan(sales), year BETWEEN 2020 AND 2023)
  with_partition_pruning(year_partition)
```

**Rules to Add:**
1. `mdx-to-sql-conversion` - Convert MDX to relational algebra
2. `hierarchy-rollup` - Optimize dimension hierarchies
3. `calculated-member-pushdown` - Move calculated members to SQL
4. `cube-materialization` - Detect cube queries that can use precomputed cubes

---

### 4. TPC-H Optimization Patterns

**TPC-H Characteristics:**
- 22 complex business intelligence queries
- Large fact tables (orders, lineitem)
- Star schema with dimension tables
- Heavy aggregations and joins
- Date range filters

**Exasol TPC-H Winners (100GB-100TB):**

**Q1: Pricing Summary Report**
```sql
SELECT
  l_returnflag, l_linestatus,
  SUM(l_quantity) as sum_qty,
  SUM(l_extendedprice) as sum_base_price,
  ...
FROM lineitem
WHERE l_shipdate <= date '1998-12-01' - interval '90' day
GROUP BY l_returnflag, l_linestatus
```

**Exasol Optimizations:**
- Partition pruning on l_shipdate
- Columnar scan with late materialization
- Parallel aggregation across cluster nodes
- SIMD vectorized SUM operations

**Rules to Add:**
```
# Date partition pruning
Filter(Scan(lineitem), l_shipdate <= '1998-09-01')
→ PartitionPrunedScan(
    lineitem,
    partitions=filter_partitions(lineitem.l_shipdate, <= '1998-09-01')
  )

# Parallel columnar aggregation
Agg(Filter(Scan(lineitem), ...), [l_returnflag, l_linestatus], [sum(l_quantity)])
→ ParallelColumnarAgg(
    LocalAgg(ColumnarScan(...), ...),
    FinalAgg(...)
  )
```

**Q3: Shipping Priority Query (Join-heavy)**
```sql
SELECT
  l_orderkey, SUM(l_extendedprice * (1 - l_discount)) as revenue,
  o_orderdate, o_shippriority
FROM customer, orders, lineitem
WHERE c_mktsegment = 'BUILDING'
  AND c_custkey = o_custkey
  AND l_orderkey = o_orderkey
  AND o_orderdate < '1995-03-15'
  AND l_shipdate > '1995-03-15'
GROUP BY l_orderkey, o_orderdate, o_shippriority
ORDER BY revenue DESC, o_orderdate
LIMIT 10
```

**Exasol Optimizations:**
- Bloom filter from customer to orders
- Colocated join (orders-lineitem on orderkey)
- Parallel hash join across nodes
- Top-K pushdown (LIMIT 10)

**Rules to Add:**
```
# Bloom filter generation
Join(
  Filter(Scan(customer), c_mktsegment = 'BUILDING'),
  Scan(orders),
  c_custkey = o_custkey
)
→ BloomFilterJoin(
    BloomFilterBuild(Filter(Scan(customer), ...), c_custkey),
    BloomFilterProbe(Scan(orders), o_custkey),
    c_custkey = o_custkey
  )

# Top-K pushdown through join
TopK(Sort(Agg(Join(...)), revenue DESC), 10)
→ TopK_PushDown(
    ParallelSort(
      LocalTopK(Agg(Join(...)), 10),
      revenue DESC
    ),
    10
  )
```

---

### 5. Compression & Encoding

**Exasol Compression:**
- Dictionary encoding for low-cardinality columns
- Run-length encoding (RLE) for sorted data
- Bit-packing for integers
- LZ4 for high-cardinality string data

**Optimization Opportunities:**
```
# Dictionary-encoded scan
Filter(Scan(orders, [o_orderstatus]), o_orderstatus = 'F')
→ DictionaryEncodedFilter(
    Scan(orders, [o_orderstatus], encoding=dict),
    dict_lookup('F') = dict_value
  )

# Run-length encoded aggregation
Count(Filter(Scan(lineitem, [l_shipdate]), l_shipdate = '2024-01-01'))
→ RLE_Count(
    Scan(lineitem, [l_shipdate], encoding=rle),
    target_value='2024-01-01'
  )
```

**Rules to Add:**
1. `dictionary-encoding-filter` - Filter on dictionary codes instead of values
2. `rle-aggregation` - Count/sum directly on RLE data
3. `bit-packing-arithmetic` - Arithmetic on compressed integers

---

### 6. Smart Shuffle Optimization

**Problem:** Data movement is expensive in distributed systems

**Exasol Strategy:**
- Minimize shuffle by smart partitioning
- Broadcast small tables instead of shuffling
- Reuse shuffled data across multiple operators

**Optimization Opportunities:**
```
# Reuse shuffle output
Join(t1, t2, key1) → Join(result1, t3, key2)
→ Shuffle(t1, t2, key1)
  → LocalJoin(t1_partition, t2_partition)
  → ReusePartition(result1, t3, key2)
    (if key2 is compatible with key1 partitioning)

# Cascade broadcast
Join(t1, t2, k1) → Join(result, t3, k2)
where size(t1) < broadcast_threshold
  AND size(t2) < broadcast_threshold
  AND size(t3) < broadcast_threshold
→ BroadcastJoin(
    BroadcastJoin(t1, t2, k1),
    t3, k2
  )  # Broadcast all small tables once
```

**Rules to Add:**
1. `shuffle-reuse` - Reuse existing partitioning when possible
2. `cascade-broadcast` - Broadcast multiple small tables efficiently
3. `partial-shuffle` - Shuffle only needed columns

---

## Proposed Rule Categories for Exasol

### Category A: In-Memory Storage Rules (High Priority)

| Rule ID | Name | Description |
|---------|------|-------------|
| EXA-001 | columnar-scan-inmem | Convert scan to columnar when data in memory |
| EXA-002 | late-materialization | Defer tuple reconstruction to last stage |
| EXA-003 | column-filter-pushdown | Push predicates to column indexes |
| EXA-004 | bloom-filter-join | Generate bloom filters for selective joins |
| EXA-005 | simd-vectorization | Tag operations for SIMD execution |

### Category B: Distributed Execution Rules (High Priority)

| Rule ID | Name | Description |
|---------|------|-------------|
| EXA-101 | parallel-aggregation | Two-phase aggregation (local + global) |
| EXA-102 | broadcast-vs-shuffle | Choose join strategy based on size |
| EXA-103 | colocated-join | Detect and exploit data colocation |
| EXA-104 | partition-pruning | Eliminate partitions at planning time |
| EXA-105 | shuffle-reuse | Reuse existing data partitioning |

### Category C: MDX Support Rules (Medium Priority)

| Rule ID | Name | Description |
|---------|------|-------------|
| EXA-201 | mdx-to-relational | Convert MDX queries to relational algebra |
| EXA-202 | hierarchy-rollup | Optimize dimension hierarchy navigation |
| EXA-203 | cube-detection | Detect queries that can use precomputed cubes |
| EXA-204 | calculated-member | Push down calculated MDX members |

### Category D: TPC-H Patterns (Medium Priority)

| Rule ID | Name | Description |
|---------|------|-------------|
| EXA-301 | date-partition-prune | Prune partitions based on date ranges |
| EXA-302 | topk-pushdown | Push LIMIT through join/agg |
| EXA-303 | selective-bloom | Bloom filter for highly selective joins |
| EXA-304 | star-join-optimization | Optimize star schema joins |

### Category E: Compression Rules (Low Priority)

| Rule ID | Name | Description |
|---------|------|-------------|
| EXA-401 | dictionary-filter | Filter on dictionary codes |
| EXA-402 | rle-aggregation | Aggregate on RLE-encoded data |
| EXA-403 | bitpacked-arithmetic | Arithmetic on compressed integers |

---

## Implementation Plan

### Phase 1: Core In-Memory Rules (1-2 weeks)
1. Implement EXA-001 through EXA-005
2. Add tests with synthetic in-memory data
3. Benchmark against standard scan/filter/join

### Phase 2: Distributed Execution (2-3 weeks)
1. Implement EXA-101 through EXA-105
2. Simulate cluster behavior in tests
3. Cost model for broadcast vs shuffle

### Phase 3: MDX Support (2-3 weeks)
1. Design MDX → RelExpr translation
2. Implement EXA-201 through EXA-204
3. Test with sample MDX queries

### Phase 4: TPC-H Optimization (1-2 weeks)
1. Implement EXA-301 through EXA-304
2. Test on TPC-H queries
3. Benchmark against PostgreSQL/DuckDB

### Phase 5: Compression (1 week)
1. Implement EXA-401 through EXA-403
2. Add encoding-aware cost model

**Total Estimated Time:** 7-11 weeks

---

## Next Steps

1. ✅ Research Exasol features (THIS DOCUMENT)
2. 🔲 Create RFC for Exasol support
3. 🔲 Implement Category A rules (in-memory storage)
4. 🔲 Add tests and benchmarks
5. 🔲 Implement Category B rules (distributed execution)
6. 🔲 Add MDX support (Category C)
7. 🔲 Optimize for TPC-H patterns (Category D)

---

## Questions for User

1. **Priority:** Which category should we implement first?
   - A: In-Memory Storage (high impact for Exasol-like workloads)
   - B: Distributed Execution (cluster support)
   - C: MDX Support (OLAP cube queries)
   - D: TPC-H Patterns (benchmark optimization)

2. **Scope:** Should we:
   - Start with one category and iterate?
   - Implement a horizontal slice across all categories?

3. **Testing:** Do you have:
   - Access to Exasol for benchmarking?
   - TPC-H dataset for testing?
   - MDX query samples?

4. **Integration:** Should Exasol rules:
   - Be always active?
   - Only activate with `--dialect=exasol` flag?
   - Auto-detect based on query patterns?

---

## Resources

**Exasol Documentation:**
- Architecture: https://docs.exasol.com/db/latest/planning/architecture.htm
- Best Practices: https://docs.exasol.com/db/latest/performance/best_practices.htm
- TPC-H Results: http://www.tpc.org/tpch/results/

**MDX Reference:**
- MDX Specification: https://en.wikipedia.org/wiki/MultiDimensional_eXpressions
- SQL Server MDX Guide: https://learn.microsoft.com/en-us/analysis-services/mdx/

**TPC-H Benchmark:**
- Queries: http://www.tpc.org/tpc_documents_current_versions/pdf/tpc-h_v3.0.1.pdf
- Schema: Star schema with fact tables (orders, lineitem)

---

**Status:** Research complete, awaiting prioritization for implementation.
