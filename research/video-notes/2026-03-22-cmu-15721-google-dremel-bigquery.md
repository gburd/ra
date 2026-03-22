# CMU 15-721 Lecture 17: System Analysis - Google Dremel / BigQuery

**Source:** CMU 15-721 Spring 2024, Lecture 17
**Date:** 2024-04-08
**Topic:** Google's Dremel/BigQuery architecture and optimization techniques
**Key Paper:** "Dremel: A Decade of Interactive SQL Analysis at Web Scale" (VLDB 2020)

## Key Points

This lecture analyzes Google's Dremel (the engine behind BigQuery), which pioneered
several optimization techniques now standard in cloud analytics databases.

### Dremel Architecture

1. **Disaggregated storage**: Data stored in Google's Colossus distributed filesystem
2. **In-situ processing**: Query data where it lives, no ETL needed
3. **Multi-level execution tree**: Root -> intermediate -> leaf servers
4. **Shuffle persistence**: Intermediate results written to distributed storage

### Optimization Techniques

**1. Tree-Structured Query Execution:**
- Queries form a multi-level tree of execution nodes
- Root server: query planning, final aggregation
- Intermediate servers: partial aggregation, data redistribution
- Leaf servers: scan and local filtering/aggregation
- Optimizer decides tree depth and fan-out based on data size

**Optimization rule:** Multi-level partial aggregation - push aggregation to each
tree level to reduce data flowing between levels.

**2. Columnar Nested Data (Dremel encoding):**
- Nested/repeated fields stored columnar with repetition and definition levels
- Enables reading individual nested fields without deserializing full records
- Optimizer must understand which nested fields are needed

**Optimization rule:** Nested column pruning - project only the nested fields
needed by the query, not the entire nested structure.

**3. Dynamic Query Execution:**
- Dremel adapts execution plans at runtime based on observed data characteristics
- Shuffle join: data redistributed by join key during execution
- Broadcast join: small table broadcast to all leaf servers
- Decision made at runtime based on actual data sizes (not estimates)

**Optimization rule:** Runtime join strategy selection - defer broadcast vs shuffle
decision until actual table sizes are known at execution time.

**4. Query Queuing and Priority:**
- Slots-based resource management
- Priority queues for different workload classes
- Optimizer considers available resources when generating plans
- Can generate less parallel plans when resources are scarce

**5. Approximate Query Processing:**
- HyperLogLog for approximate COUNT DISTINCT
- Approximate percentiles using T-Digest
- Optimizer can substitute approximate operators when user opts in

**Optimization rule:** approximate-aggregate-substitution - when approximate results
are acceptable, replace exact aggregates with probabilistic data structures.

### BigQuery-Specific Optimizations

1. **Partition pruning**: Skip partitions that don't match WHERE clause
2. **Clustering pruning**: Skip data blocks based on clustering key order
3. **Materialized view matching**: Automatically rewrite queries to use MVs
4. **BI Engine**: In-memory cache for frequently queried datasets
5. **Slot autoscaling**: Dynamically adjust compute resources during query execution

### Nested and Semi-Structured Data

BigQuery's handling of nested/repeated fields reveals optimization opportunities:
- UNNEST operations can be pushed down or deferred
- Aggregation over nested fields can avoid full UNNEST
- Join on nested field can use nested-to-flat conversion

## Optimization Rules for Ra

### New Rules Identified

1. **multi-level-aggregation** - For tree-structured execution, insert partial
   aggregation at each level of the execution tree
2. **nested-column-pruning** - Project only referenced nested/repeated fields
3. **runtime-join-strategy-deferral** - Generate plans where join strategy
   (broadcast vs hash) is decided at runtime based on actual sizes
4. **approximate-aggregate-substitution** - Replace exact COUNT DISTINCT with
   HyperLogLog when user allows approximation
5. **clustering-key-pruning** - Skip data blocks based on clustering key ranges
   (generalization of zone map pruning to clustered data)
6. **materialized-view-rewrite** - Automatically match and rewrite queries to
   use available materialized views
7. **unnest-pushdown** - Push UNNEST operation down to minimize rows expanded
   (UNNEST after filter reduces expansion)
8. **nested-aggregate-without-unnest** - Compute aggregates over nested arrays
   without full UNNEST when possible

### Ra Gap Analysis

Ra currently has:
- `rules/unnest/` - Unnest rules (recently added)
- `rules/physical/materialization/` - Materialization rules
- No approximate query processing rules
- No materialized view rewriting
- No clustering-key-aware pruning

**Missing capabilities:**
- Multi-level aggregation for tree execution
- Approximate aggregate operators (HLL, T-Digest)
- Materialized view matching and rewriting
- Nested/semi-structured data optimization
- Runtime strategy deferral mechanism

## Relevance to Ra

**Priority:** Medium-High - Several of these techniques (materialized view rewriting,
approximate aggregation, multi-level aggregation) are broadly applicable beyond
Dremel/BigQuery. Materialized view rewriting alone could be a high-impact RFC.

**Proposed RFCs:**
1. Materialized View Matching and Rewriting
2. Approximate Aggregation Framework (HLL, approximate percentiles)
