# CMU 15-721 Lecture 18: System Analysis - Databricks/Spark

**Source:** CMU 15-721 Spring 2024, Lecture 18
**Speaker:** Andy Pavlo (with guest from Databricks)
**Topic:** Databricks/Spark SQL Optimization

## Key Concepts

### Catalyst Optimizer
- Rule-based optimizer with extensible rule framework
- Rules organized in batches, each batch runs until fixed point
- Batch ordering: analysis -> logical optimization -> physical planning
- Written in Scala using pattern matching on logical plan trees

### Key Optimization Rules

#### Adaptive Query Execution (AQE)
- Collects runtime statistics at shuffle boundaries
- Re-optimizes remaining plan with actual row counts
- Three key adaptations:
  1. **Coalescing post-shuffle partitions**: Merge small partitions
  2. **Converting sort-merge join to broadcast join**: When one side < threshold
  3. **Optimizing skew joins**: Split skewed partitions across workers

#### Dynamic Partition Pruning
- When fact table joined to dimension table with filter:
  1. Execute dimension table filter first
  2. Collect distinct join key values
  3. Push as IN-list filter to fact table scan
  4. Prune fact table partitions that don't contain matching keys
- Huge speedup for star schema queries (10-100x)

#### Cost-Based Optimizer (CBO)
- Activated with ANALYZE TABLE to collect statistics
- Uses statistics for: join ordering, join type selection, aggregate strategy
- Column statistics: distinct count, min, max, avg_len, histogram
- Table statistics: row count, size in bytes

#### Whole-Stage Code Generation
- Compile entire pipeline stages to single Java method
- Eliminates virtual function call overhead
- 10-100x faster than interpreted Volcano execution
- Adaptive: interpret small queries, compile large ones

## Applicable to Ra

### New Rule Ideas
1. **Adaptive Join Strategy Switching**: At shuffle boundary, if one side
   is much smaller than estimated, switch from sort-merge to broadcast join.
2. **Skew-Aware Join Partitioning**: Detect skewed join keys and split
   heavy partitions across multiple workers.
3. **Dynamic Partition Pruning via Dimension Filter**: For star schema,
   collect dimension join keys first, use as scan filter on fact table.
4. **Post-Shuffle Partition Coalescing**: Merge small partitions after
   shuffle to avoid many-small-partition overhead.
5. **Compilation Threshold Rule**: Choose interpret vs compile based on
   estimated execution time (compile if > 100ms expected).
6. **Rule Batch Fixed-Point**: Apply rule batches until no changes,
   handling rule interactions automatically.

### Gap Analysis
- Ra has experimental adaptive rules (24 total)
- Missing: AQE-style mid-query re-optimization
- Missing: dynamic partition pruning
- Missing: skew detection and handling
- Missing: compilation threshold decisions
- Missing: post-shuffle partition optimization
