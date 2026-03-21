# CMU 15-721 Lecture 19: System Analysis - Snowflake

**Source:** CMU 15-721 Spring 2024, Lecture 19
**Speaker:** Andy Pavlo (with guest from Snowflake)
**Topic:** Snowflake Architecture and Optimization

## Key Concepts

### Snowflake Optimizer Architecture
- Cascades-style top-down optimizer
- Property-driven optimization (ordering, distribution, partitioning)
- Multi-phase: rewrite rules -> cost-based optimization -> physical planning
- Handles distributed query planning across compute clusters

### Key Optimization Techniques

#### Pruning
- **Micro-partition pruning**: Skip files based on column min/max metadata
- **Dynamic pruning**: Runtime filters from hash join build side
- **Clustering-based pruning**: Data physically organized by cluster key
- Critical for large analytical tables (TB+ scale)

#### Runtime Filters
- Hash join build phase produces bloom filter on join key
- Bloom filter pushed to probe side scan
- Particularly effective for star schema (fact table scan reduced 10-100x)
- Also pushed across exchanges in distributed plans

#### Adaptive Execution
- Monitor cardinality at materialization points
- Re-optimize downstream plan if estimate is off by > 10x
- "Adaptive redistribution": change distribution strategy mid-query
- Critical for multi-join queries where estimation compounds

#### Result Caching
- Query result cache: identical query returns cached result
- Metadata cache: table statistics cached across queries
- Plan cache: compiled query plans reused for parameterized queries
- Cache invalidation tied to data modification timestamps

### Cost Model Features
- Hardware-aware: different cost weights per warehouse size
- Network cost dominates for distributed joins
- I/O cost includes cloud storage latency (S3/Azure/GCS)
- Distinct cost model for different storage tiers (hot/warm/cold)

## Applicable to Ra

### New Rule Ideas
1. **Micro-Partition Pruning Rule**: Use column min/max from file metadata
   to skip entire files/partitions that can't contain matching rows.
2. **Dynamic Filter Generation**: Generate bloom filters from hash join
   build side and push to scan operators.
3. **Adaptive Redistribution Rule**: When distributed join detects skew
   at runtime, switch from hash to broadcast distribution.
4. **Result Cache Matching**: When identical subquery appears multiple
   times in a query, compute once and cache.
5. **Cloud Storage Cost Model**: Different I/O costs for local vs
   cloud storage (latency, throughput, pricing).
6. **Cluster Key Awareness**: Factor data clustering into scan cost
   and predicate selectivity estimation.

### Gap Analysis
- Ra has some pruning rules in distributed/ directory
- Missing: runtime filter / dynamic pruning infrastructure
- Missing: adaptive re-optimization mid-query
- Missing: result caching at subquery level
- Missing: cloud storage cost model differentiation
