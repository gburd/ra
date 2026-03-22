# CMU 15-721 Lecture 8: Scheduling, Coordination, and Morsel-Driven Parallelism

**Source:** CMU 15-721 Spring 2024, Lecture 8
**Date:** 2024-02-19
**Topic:** Query scheduling, parallelism frameworks, and NUMA-aware execution
**Key Papers:** Morsel-Driven Parallelism (SIGMOD 2014), Self-Tuning Query Scheduling

## Key Points

This lecture covers how modern databases schedule and coordinate parallel query
execution. The optimizer's role extends beyond plan selection to include parallelism
decisions and resource allocation.

### Morsel-Driven Parallelism (Leis et al., HyPer)

**Core concept:** Divide input data into fixed-size "morsels" (typically 10K-100K rows),
assign morsels to worker threads dynamically.

**Key design decisions the optimizer must make:**

1. **Pipeline boundaries**: Identify where data must materialize (hash table build,
   sort, group-by aggregation). These are "pipeline breakers."
2. **Pipeline parallelism**: Within each pipeline, all operators execute on the same
   morsel without materialization (push-based, compiled).
3. **NUMA-aware scheduling**: Assign morsels to threads on the NUMA node where the
   data resides. Avoid cross-node memory access.

**Optimizer rules needed:**
- Identify pipeline-breaking operators in the plan
- Decide degree of parallelism per pipeline stage
- Place exchange (shuffle/broadcast) operators for data redistribution
- Determine morsel size based on cache hierarchy

### Exchange Operator Placement

The optimizer must decide where to insert exchange operators:

1. **Gather**: Collect results from parallel workers to a single stream
2. **Scatter (Hash)**: Redistribute data by hash of join/group key
3. **Broadcast**: Send all data to all workers (for small tables in joins)
4. **Range partition**: Distribute data by key ranges (for merge join)

**Placement rules:**
- Insert Gather before final result delivery
- Insert Hash exchange before hash join build side when data not co-partitioned
- Insert Broadcast for small dimension tables in star schema joins
- Insert Range partition before parallel merge join
- Avoid unnecessary exchanges (data already correctly partitioned)

### Worker Allocation Strategies

**Static allocation:** Assign fixed number of workers per query at compile time.
- Simple but cannot adapt to system load
- Risk of over-provisioning or under-provisioning

**Elastic allocation:** Adjust worker count at runtime based on system utilization.
- Monitor CPU/memory usage, steal workers from idle queries
- Snowflake, Databricks, BigQuery all use elastic allocation

**Optimizer rule:** Generate plans with "parallelism hints" that specify minimum
and maximum useful degree of parallelism per pipeline stage. Runtime scheduler
allocates workers within these bounds.

### Self-Tuning Query Scheduling

Automatic tuning of scheduling parameters based on workload characteristics:
- Morsel size: larger morsels reduce scheduling overhead, smaller morsels improve
  load balancing
- Pipeline priority: execute pipelines that produce data for downstream operators first
- Memory budget: limit concurrent pipeline stages to avoid memory pressure

## Optimization Rules for Ra

### New Rules Identified

1. **pipeline-breaker-annotation** - Annotate plan nodes as pipeline-breaking (hash
   build, sort, aggregate) or pipelined (filter, project, probe)
2. **exchange-operator-placement** - Insert exchange operators at optimal points in
   parallel plans based on data distribution requirements
3. **broadcast-vs-hash-decision** - Choose broadcast for small tables (< threshold)
   vs hash redistribution for large tables in parallel joins
4. **numa-locality-aware-scan** - Assign scan ranges to workers based on NUMA node
   affinity of the data pages
5. **parallelism-degree-estimation** - Estimate useful degree of parallelism per
   pipeline stage based on data size and operator cost
6. **pipeline-fusion** - Merge adjacent pipelined operators into a single pipeline
   stage to reduce materialization overhead
7. **memory-budget-partitioning** - Allocate memory budget across concurrent pipeline
   stages (hash tables, sort buffers) to prevent spilling

### Ra Gap Analysis

Ra currently has:
- `rules/execution-models/morsel-driven/` - Morsel-driven execution rules
- `rules/execution-models/pipeline/` - Pipeline execution rules
- `rules/execution-models/push-based/` - Push-based execution rules
- `rules/physical/parallelization/` - Parallelization rules
- `rules/parallel/` - Parallel execution rules

**Likely already covered (verify):**
- Basic pipeline identification
- Parallel scan partitioning
- Exchange operator basics

**Missing capabilities:**
- NUMA-aware morsel scheduling
- Pipeline fusion optimization
- Exchange operator placement optimization (broadcast vs hash vs range)
- Memory budget allocation across concurrent pipelines
- Elastic parallelism degree adjustment
- Pipeline priority scheduling

## Relevance to Ra

**Priority:** Medium-High - Parallelism decisions are critical for performance on
modern multi-core hardware. Ra has the framework but may lack the fine-grained
scheduling optimization rules.

**Proposed RFC:** Exchange Operator Optimization - formalize the rules for choosing
between broadcast, hash, and range partitioning at exchange points, incorporating
data size, skew, and NUMA topology into the decision.
