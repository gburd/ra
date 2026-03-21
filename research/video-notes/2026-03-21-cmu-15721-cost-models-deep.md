# CMU 15-721 Lecture 16: Cost Models (Deep Analysis)

**Source:** CMU 15-721 Spring 2024, Lecture 16
**Speaker:** Andy Pavlo
**Topic:** Cost Models for Query Optimization

## Key Concepts

### Cost Model Components
1. **CPU cost**: tuple processing, predicate evaluation, hash computation
2. **I/O cost**: sequential page reads, random page reads, index page reads
3. **Memory cost**: hash table size, sort buffers, aggregate state
4. **Network cost**: data shuffling, broadcast, gather (distributed only)

### Startup vs Total Cost (PostgreSQL Model)
- Startup cost: work before first row produced
- Total cost: work to produce all rows
- Run cost: total - startup
- Critical for LIMIT queries and pipelined execution
- Sort: high startup (O(n log n)), zero run cost
- SeqScan: zero startup, proportional run cost
- Hash join build: high startup, probe is run cost

### Hardware Calibration
- Default cost parameters rarely match actual hardware
- SSD vs HDD: random I/O cost differs by 100x
- NVMe: sequential/random gap much smaller
- Calibration approaches:
  - Manual: DBA sets parameters (PostgreSQL approach)
  - Micro-benchmark: Run small I/O and CPU tests on startup
  - Adaptive: Learn from execution feedback

### Cardinality Estimation Errors
- Leis et al. 2015: "How Good Are Query Optimizers, Really?"
- Estimation errors compound multiplicatively through joins
- 2x per-join error -> 2^N for N-table join
- Independence assumption is the primary source of error
- Multi-column statistics help but are expensive to maintain

### Cost Model Pitfalls
1. **Correlation between columns**: independence assumption fails
2. **Data skew**: uniform distribution assumption fails
3. **Stale statistics**: table changed since last ANALYZE
4. **Parameter sniffing**: prepared statement uses wrong plan for different parameters
5. **Costing string operations**: LIKE patterns, regex have unpredictable cost
6. **Function costs**: UDFs have unknown selectivity and execution cost
7. **Compression**: compressed data changes I/O vs CPU tradeoff

## Specific Cost Formulas

### Sequential Scan
```
cost = cpu_tuple_cost * rows + seq_page_cost * pages
startup = 0
```

### Index Scan
```
cost = (cpu_index_tuple_cost + cpu_tuple_cost) * rows
     + random_page_cost * index_pages * (1 - correlation^2)
     + seq_page_cost * index_pages * correlation^2
startup = 0 (pipelined)
```

### Hash Join
```
build_cost = cpu_tuple_cost * inner_rows + hash_cost * inner_rows
probe_cost = cpu_tuple_cost * outer_rows + hash_cost * outer_rows
startup = build_cost (blocking on build side)
total = build_cost + probe_cost
memory = hash_table_size(inner_rows)
```

### Sort
```
comparison_cost = 2 * cpu_operator_cost
sort_cost = comparison_cost * rows * log2(rows)
startup = sort_cost (blocking)
total = sort_cost + cpu_tuple_cost * rows
```

### Nested Loop
```
startup = outer.startup + inner.startup
total = outer.total + outer.rows * inner.total
```

### Merge Join
```
startup = outer.startup + inner.startup (both must be sorted)
total = merge_cost * (outer.rows + inner.rows)
  + outer.sort_cost + inner.sort_cost (if not already sorted)
```

## Applicable to Ra

### New Rule Ideas
1. **Correlation-Aware Index Cost**: Use column correlation to estimate sequential
   vs random I/O for index scans. High correlation = mostly sequential.
2. **Compression-Aware Cost**: When data is compressed, reduce I/O cost but
   increase CPU cost proportionally.
3. **Function Cost Estimation**: Assign default costs to common functions
   (string ops = 10x, regex = 100x, UDF = 1000x baseline).
4. **Parameter Sensitivity Detection**: Flag plans that are sensitive to
   parameter values (cost varies > 10x across parameter range).
5. **Memory Spill Threshold**: When hash table exceeds memory, cost model
   must account for spill-to-disk overhead (2x I/O for write + read).
6. **Cache-Aware Random I/O**: Reduce effective random_page_cost based on
   working set size vs available cache (effective_cache_size).

### Gap Analysis
- Ra has CPU/IO/network/memory cost model (good foundation)
- Ra now has startup vs total cost (recently added)
- Missing: correlation-aware index costing
- Missing: compression-aware cost adjustments
- Missing: memory pressure / spill modeling
- Missing: adaptive calibration from execution feedback
