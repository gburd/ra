# RA Optimizer Rule Index

**Total rules: 969** across 84 directories

Auto-generated rule index for the RA query optimizer. Each rule is a `.rra`
literate programming file containing egg rewrite rules, SQL test cases,
and academic references.

## Table of Contents

- [cost-models/](#user-content-cost-models) (38 rules)
  - system-r/ (11)
- [database-specific/](#user-content-database-specific) (356 rules)
  - calcite/ (48)
  - clickhouse/ (37)
  - cockroachdb/ (19)
  - datafusion/ (20)
  - derby/ (22)
  - drill/ (4)
  - duckdb/ (2)
  - flink/ (7)
  - greenplum/ (2)
  - hyper/ (3)
  - impala/ (4)
  - materialize/ (21)
  - monetdb/ (28)
  - mongodb/ (19)
  - mssql/ (20)
  - mysql/ (25)
  - neo4j/ (17)
  - oracle/ (20)
  - postgresql/ (2)
  - presto/ (3)
  - sqlite/ (2)
  - tidb/ (19)
  - timescaledb/ (3)
  - trino/ (6)
  - voltdb/ (3)
- [distributed/](#user-content-distributed) (58 rules)
  - colocation/ (6)
  - coprocessor-pushdown/ (2)
  - data-movement/ (6)
  - distributed-joins/ (14)
  - distributed-sort/ (4)
  - distributed-transactions/ (2)
  - exchange-placement/ (5)
  - locality-optimization/ (4)
  - partial-aggregation/ (6)
  - partition-pruning/ (5)
  - stage-planning/ (4)
- [execution-models/](#user-content-execution-models) (99 rules)
  - adaptive/ (11)
  - column-at-a-time/ (17)
  - differential/ (18)
  - experimental/ (8)
  - morsel-driven/ (13)
  - push-based/ (10)
  - vectorized/ (12)
  - volcano/ (10)
- [experimental/](#user-content-experimental) (46 rules)
  - adaptive/ (13)
  - approximate/ (3)
  - compilation/ (2)
  - hardware-accel/ (2)
  - ml-guided/ (9)
  - semantic/ (7)
  - wcoj/ (10)
- [hardware/](#user-content-hardware) (21 rules)
  - accelerator/ (5)
  - data-placement/ (4)
  - fpga/ (4)
  - gpu/ (8)
- [logical/](#user-content-logical) (209 rules)
  - aggregate-pushdown/ (22)
  - cte-optimization/ (5)
  - distinct-elimination/ (5)
  - expression-simplification/ (10)
  - function-optimization/ (58)
  - join-elimination/ (19)
  - join-reordering/ (9)
  - limit-pushdown/ (12)
  - predicate-pushdown/ (17)
  - projection-pushdown/ (7)
  - semantic-rewriting/ (8)
  - set-operations/ (10)
  - sideways-information-passing/ (3)
  - subquery-unnesting/ (17)
  - view-rewriting/ (2)
  - window-pushdown/ (5)
- [multi-model/](#user-content-multi-model) (30 rules)
  - document/ (10)
  - graph/ (10)
  - timeseries/ (10)
- [physical/](#user-content-physical) (108 rules)
  - access-path-selection/ (4)
  - aggregation-strategies/ (16)
  - index-selection/ (36)
  - join-algorithms/ (18)
  - materialization/ (13)
  - optimizer-framework/ (5)
  - parallelization/ (16)

## Summary

| Directory | Subdirectories | Rules | Description |
|-----------|---------------:|------:|-------------|
| `cost-models/` | 2 | 38 | Cost estimation models and statistics |
| `database-specific/` | 25 | 356 | Rules mined from specific database systems |
| `distributed/` | 11 | 58 | Distributed query processing rules |
| `execution-models/` | 8 | 99 | Execution engine strategies |
| `experimental/` | 7 | 46 | Experimental and research rules |
| `hardware/` | 4 | 21 | Hardware-aware optimization rules |
| `logical/` | 16 | 209 | Logical query rewrite rules |
| `multi-model/` | 3 | 30 | Multi-model (document, graph, timeseries) |
| `physical/` | 7 | 108 | Physical operator selection rules |
| **Total** | **84** | **969** | |

## Rules by Directory

### cost-models/ (38 rules)

Cost estimation models and statistics.

#### `cost-models/` (27 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `aggregate-cardinality-estimation` | Aggregate Cardinality Estimation | O(|group-by columns|) |
| `aggregation-cost-formulas` | Aggregation Cost Modeling | O(1) per estimate |
| `cache-conscious-cost-model` | Cache-Conscious Algorithm Selection Cost Model | O(1) per algorithm selection |
| `cardinality-estimation` | Output Cardinality Estimation | O(|predicates|) per operator |
| `composite-cost-model` | Composite Cost Model | O(1) per operator |
| `cost-calibration` | Runtime Cost Model Calibration | O(n) calibration queries |
| `cost-calibration-duckdb` | DuckDB Cost Model Calibration | O(1) parameter lookup |
| `cost-calibration-postgresql` | PostgreSQL Cost Model Calibration | O(1) parameter lookup |
| `cpu-cost-model` | CPU Cost Model | O(1) per operator |
| `distributed-query-routing-cost` | Distributed Query Routing and Data Movement Cost | O(nodes * operators) per plan |
| `geospatial-cost-model` | Geospatial Query Cost Model | O(log n + k) for spatial index queries |
| `gpu-cost-model` | GPU Offloading Cost Model | O(1) per operator placement decision |
| `histogram-based-estimation` | Histogram-Based Cardinality Estimation | O(log B) per query, B = bucket count |
| `io-cost-model` | I/O Cost Model for Storage Access | O(1) per operator |
| `join-cardinality-estimation` | Join Cardinality Estimation | O(|join predicates|) |
| `join-cost-formulas` | General Join Cost Modeling | O(1) per estimate |
| `memory-cost-model` | Memory and Cache Hierarchy Cost Model | O(1) per operator |
| `multi-column-correlation` | Multi-Column Correlation Modeling | O(k^2) for k correlated columns |
| `network-cost-model` | Network Cost Model for Distributed Queries | O(1) per data movement operator |
| `numa-aware-cost-model` | NUMA-Aware Operator Placement Cost Model | O(sockets) per operator placement |
| `outlier-aware-estimation` | Outlier-Aware Cost Estimation | O(n) for detection, O(1) per query |
| `sampling-based-estimation` | Sampling-Based Cardinality Estimation | O(sample_size) per estimate |
| `selectivity-estimation` | Predicate Selectivity Estimation | O(|predicates|) |
| `system-r-cost-formula` | System R Cost Formula | O(1) |
| `system-r-join-cardinality` | System R Join Cardinality Estimation | O(1) |
| `system-r-selectivity-formulas` | System R Selectivity Estimation Formulas | O(1) |
| `time-series-cost-model` | Time-Series Specific Cost Model | O(1) per estimation with temporal statistics |

#### `cost-models/system-r/` (11 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `system-r-access-path-enumeration` | System R Access Path Enumeration Strategy | O(2^n * |paths| * |orders|) |
| `system-r-dp-join-order` | System R Dynamic Programming Join Ordering | O(2^n) for n relations |
| `system-r-independence-assumption` | System R Predicate Independence Assumption | O(1) |
| `system-r-index-only-access` | System R Index-Only Access (Covering Index) | O(F * index_pages) |
| `system-r-index-vs-scan` | System R Index vs Sequential Scan Selection | O(|indexes|) |
| `system-r-interesting-orders` | System R Interesting Orders | O(2^n * |orders|) |
| `system-r-left-deep-trees` | System R Left-Deep Tree Restriction | O(n!) reduced from O(Catalan(n)) |
| `system-r-merge-scan-cost` | System R Sort-Merge (Merge-Scan) Join Cost | O(n log n + m log m + n + m) |
| `system-r-multi-index-access` | System R Multi-Index Access Paths | O(F1*N + F2*N) |
| `system-r-nested-loop-cost` | System R Nested-Loop Join Cost Formulas | O(n * m) or O(n * B_tree_height) |
| `system-r-temp-materialization` | System R Temporary Relation Materialization Cost | O(n) write + O(n) read |

### database-specific/ (356 rules)

Rules mined from specific database systems.

#### `database-specific/calcite/` (48 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `aggregate-merge` | Aggregate Merge | O(1) |
| `aggregate-reduce-functions` | Aggregate Reduce Functions | O(1) |
| `aggregate-values` | Aggregate Values | O(n) |
| `calcite-aggregate-expand-distinct` | Calcite AggregateExpandDistinctAggregatesRule | O(n) |
| `calcite-aggregate-join-transpose` | Calcite AggregateJoinTransposeRule | O(n) |
| `calcite-aggregate-project-merge` | Calcite AggregateProjectMergeRule | O(n) |
| `calcite-aggregate-remove` | Calcite AggregateRemoveRule | O(1) |
| `calcite-aggregate-union-transpose` | Calcite AggregateUnionTransposeRule | O(n) |
| `calcite-calc-merge` | Calcite CalcMergeRule | O(n) |
| `calcite-filter-aggregate-transpose` | Calcite FilterAggregateTransposeRule | O(n) |
| `calcite-filter-into-join` | Calcite FilterJoinRule | O(n) |
| `calcite-filter-project-transpose` | Calcite FilterProjectTransposeRule | O(1) |
| `calcite-filter-set-op-transpose` | Calcite FilterSetOpTransposeRule | O(n) |
| `calcite-filter-to-calc` | Calcite FilterToCalcRule | O(1) |
| `calcite-join-associate` | Calcite JoinAssociateRule | O(1) |
| `calcite-join-commute` | Calcite JoinCommuteRule | O(1) |
| `calcite-join-condition-push` | Calcite JoinConditionPushRule | O(n) |
| `calcite-join-push-through-join` | Calcite JoinPushThroughJoinRule | O(1) |
| `calcite-join-to-semi-join` | Calcite JoinToSemiJoinRule | O(1) |
| `calcite-project-merge` | Calcite ProjectMergeRule | O(n) |
| `calcite-project-remove` | Calcite ProjectRemoveRule | O(1) |
| `calcite-project-set-op-transpose` | Calcite ProjectSetOpTransposeRule | O(n) |
| `calcite-project-to-calc` | Calcite ProjectToCalcRule | O(1) |
| `calcite-project-window-transpose` | Calcite ProjectWindowTransposeRule | O(n) |
| `calcite-reduce-expressions` | Calcite ReduceExpressionsRule | O(n) |
| `calcite-sort-join-transpose` | Calcite SortJoinTransposeRule | O(1) |
| `calcite-sort-project-transpose` | Calcite SortProjectTransposeRule | O(1) |
| `calcite-sort-remove` | Calcite SortRemoveRule | O(1) |
| `calcite-sort-union-transpose` | Calcite SortUnionTransposeRule | O(1) |
| `calcite-sub-query-remove` | Calcite SubQueryRemoveRule | O(n) |
| `calcite-union-merge` | Calcite UnionMergeRule | O(1) |
| `exchange-remove-constant-keys` | Exchange Remove Constant Keys | O(1) |
| `filter-merge` | Filter Merge | O(1) |
| `filter-sample-transpose` | Filter Sample Transpose | O(1) |
| `filter-window-transpose` | Filter Window Transpose | O(1) |
| `intersect-to-distinct` | Intersect to Distinct | O(n log n) |
| `intersect-to-semi-join` | Intersect to Semi-Join | O(n log m) |
| `join-derive-is-not-null-filter` | Join Derive IsNotNull Filter | O(1) |
| `join-expand-or-to-union` | Join Expand OR to Union | O(1) |
| `join-to-multi-join` | Join to MultiJoin | O(1) |
| `join-union-transpose` | Join Union Transpose | O(1) |
| `project-correlate-transpose` | Project Correlate Transpose | O(1) |
| `project-to-window` | Project to Window | O(n log n) |
| `semi-join-filter-transpose` | Semi-Join Filter Transpose | O(1) |
| `sort-remove-duplicate-keys` | Sort Remove Duplicate Keys | O(1) |
| `union-eliminator` | Union Eliminator | O(1) |
| `union-pull-up-constants` | Union Pull Up Constants | O(1) |
| `values-reduce` | Values Reduce | O(n) |

#### `database-specific/clickhouse/` (37 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `clickhouse-aggregate-projection` | Use Aggregate Projection for GROUP BY | O(m) |
| `clickhouse-aggregate-projection-rewrite` | ClickHouse Aggregate Projection Rewrite | O(k) |
| `clickhouse-column-pruning-unused-removal` | ClickHouse Column Pruning and Unused Column Removal | O(n) |
| `clickhouse-deferred-function-execution` | ClickHouse Deferred Function Execution After Sort | O(n) |
| `clickhouse-direct-text-index-read` | Direct Read from Text/Full-Text Index | O(m) |
| `clickhouse-filter-pushdown-through-join` | Push Filter Below Join | O(n_filtered) |
| `clickhouse-filter-pushdown-to-storage` | ClickHouse Filter Pushdown to Storage Layer | O(n) |
| `clickhouse-join-to-in-conversion` | ClickHouse JOIN to IN Subquery Conversion | O(n + m) |
| `clickhouse-lazy-materialization` | ClickHouse Lazy Column Materialization | O(n) |
| `clickhouse-lift-up-array-join` | Lift ARRAY JOIN Above Filter | O(n) |
| `clickhouse-lift-up-functions` | Lift Functions Above Aggregation | O(1) |
| `clickhouse-limit-pushdown` | ClickHouse LIMIT Pushdown | O(1) |
| `clickhouse-limit-pushdown-distributed` | Push LIMIT Below Remote Exchange | O(limit * n_nodes) |
| `clickhouse-merge-expressions` | ClickHouse Expression and Filter Merging | O(1) |
| `clickhouse-normal-projection-rewrite` | ClickHouse Normal Projection Rewrite | O(n) |
| `clickhouse-normal-projection-usage` | Use Normal Projection for Column Subset | O(m) |
| `clickhouse-optimize-join-to-semi` | Convert ANY JOIN to Semi-Join | O(n) |
| `clickhouse-outer-join-to-inner` | Convert Outer Join to Inner Join | O(1) |
| `clickhouse-outer-to-inner-join-conversion` | ClickHouse Outer JOIN to Inner JOIN Conversion | O(1) |
| `clickhouse-partition-independent-aggregation` | ClickHouse Partition-Independent Aggregation | O(n/p) |
| `clickhouse-partition-pruning` | ClickHouse Partition Pruning | O(p) |
| `clickhouse-prewhere-filter-pushdown` | ClickHouse PREWHERE Filter Pushdown | O(n) |
| `clickhouse-prewhere-pushdown` | Filter to PREWHERE Optimization | O(n_filtered) |
| `clickhouse-primary-key-condition-limit` | Optimize Primary Key Condition with Limit | O(log n) |
| `clickhouse-read-in-order` | Read-in-Order Optimization for Sorting | O(n) |
| `clickhouse-read-in-order-sort-elimination` | ClickHouse Read-in-Order Sort Elimination | O(n) |
| `clickhouse-redundant-sort-removal` | ClickHouse Redundant Sort Removal | O(1) |
| `clickhouse-remove-redundant-distinct` | Remove Redundant DISTINCT | O(1) |
| `clickhouse-remove-redundant-sorting` | Remove Redundant Sorting | O(1) |
| `clickhouse-runtime-filter-join` | Runtime Filter for Join Optimization | O(n) |
| `clickhouse-runtime-join-filter` | ClickHouse Runtime Join Bloom Filter | O(n + m) |
| `clickhouse-sparse-index-granule-pruning` | ClickHouse Sparse Index Granule Pruning | O(log n) |
| `clickhouse-sparse-index-skip` | Sparse Primary Index for Granule Skipping | O(log(n/g)) |
| `clickhouse-split-filter` | Split Conjunctive Filter Predicates | O(n) |
| `clickhouse-topk-optimization` | Top-K Optimization with Priority Queue | O(n log k) |
| `clickhouse-topk-sort-optimization` | ClickHouse Top-K Sort Optimization | O(n) |
| `clickhouse-use-data-parallel-aggregation` | Use Data-Parallel Aggregation | O(n/p) |

#### `database-specific/cockroachdb/` (19 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `cockroachdb-anti-join-disjunction-to-union` | Split Disjunctive Anti-Join into Intersection | O(n1 + n2) |
| `cockroachdb-commute-left-to-right-join` | Commute Left Join to Right Join | O(1) |
| `cockroachdb-convert-semi-to-inner-non-equality` | Convert Semi-Join to Inner (Non-Equality) | O(n log n) |
| `cockroachdb-generate-index-scans` | Generate Secondary Index Scans | O(log n) |
| `cockroachdb-generate-inverted-index-scans` | Generate Inverted Index Scans (JSON/Array) | O(m) |
| `cockroachdb-generate-limited-index-scans` | Generate Limited Index Scans | O(limit) |
| `cockroachdb-locality-optimized-lookup-join` | Locality-Optimized Lookup Join | O(n_local) |
| `cockroachdb-locality-optimized-scan` | Locality-Optimized Multi-Region Scan | O(n_local) |
| `cockroachdb-push-limit-into-project-scan` | Push Limit Through Project Into Scan | O(limit) |
| `cockroachdb-push-limit-into-scan` | Push Limit Into Filtered Scan | O(limit) |
| `cockroachdb-reorder-joins` | Join Reordering (Bushy Plans) | varies |
| `cockroachdb-replace-min-with-limit` | Replace MIN GroupBy with Limit (0-1 Groups) | O(log n) |
| `cockroachdb-scalar-min-max-to-limit` | Scalar MIN/MAX Aggregation to Limit | O(log n) |
| `cockroachdb-scalar-min-max-to-subqueries` | Replace Multiple MIN/MAX with Scalar Subqueries | O(n_aggs * log n) |
| `cockroachdb-semi-join-to-inner-with-distinct` | Semi Join to Inner Join with Distinct (CockroachDB) | O(n log n) |
| `cockroachdb-split-disjunction-join-to-union` | Split Disjunctive Join Conditions into Union | O(n1 + n2) |
| `crdb-filter-consolidation` | Filter Constraint Consolidation | O(n) |
| `crdb-join-reorder` | CockroachDB Join Reordering | O(n!) |
| `crdb-lookup-join-virtual-cols` | Lookup Join with Virtual Computed Columns | O(n) |

#### `database-specific/datafusion/` (20 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `datafusion-aggregate-statistics` | DataFusion Aggregate Statistics Optimization | O(1) |
| `datafusion-common-subexpr-elimination` | DataFusion Common Subexpression Elimination | O(n^2) |
| `datafusion-decorrelate-subquery` | DataFusion Decorrelate Predicate Subquery | O(n) |
| `datafusion-eliminate-unnecessary-sort` | DataFusion Eliminate Unnecessary Sort | O(n) |
| `datafusion-filter-pushdown-through-join` | DataFusion Filter Pushdown Through Join | O(n) |
| `datafusion-hash-join-selection` | DataFusion Hash Join vs Sort-Merge Join Selection | O(n) |
| `datafusion-join-reordering` | DataFusion Join Reordering | O(2^n) |
| `datafusion-limit-pushdown` | DataFusion Limit Pushdown | O(n) |
| `datafusion-parquet-predicate-pushdown` | DataFusion Parquet Predicate Pushdown | O(n) |
| `datafusion-pipeline-breaking-optimization` | DataFusion Pipeline Breaking Optimization | O(n) |
| `datafusion-projection-pushdown` | DataFusion Projection Pushdown | O(n) |
| `datafusion-propagate-empty-relation` | DataFusion Propagate Empty Relation | O(n) |
| `datafusion-repartition-insertion` | DataFusion Repartition Insertion | O(n) |
| `datafusion-replace-distinct-aggregate` | DataFusion Replace DISTINCT with Aggregate | O(n) |
| `datafusion-scalar-subquery-to-join` | DataFusion Scalar Subquery to Join | O(n) |
| `datafusion-simplify-expressions` | DataFusion Simplify Expressions | O(n) |
| `datafusion-single-distinct-to-groupby` | DataFusion Single Distinct Aggregation to Group By | O(n) |
| `datafusion-type-coercion` | DataFusion Type Coercion Optimization | O(n) |
| `datafusion-unnest-rewrite` | DataFusion Unnest/Flatten Rewrite | O(n * m) |
| `datafusion-window-function-optimization` | DataFusion Window Function Optimization | O(n log n) |

#### `database-specific/derby/` (22 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `derby-bulk-fetch` | Apache Derby Bulk Fetch Optimization | O(n) |
| `derby-constant-expression-evaluation` | Apache Derby Constant Expression Evaluation | O(1) |
| `derby-constraint-based-optimization` | Apache Derby Constraint-Based Query Optimization | O(1) |
| `derby-covering-index` | Apache Derby Covering Index Scan | O(n) |
| `derby-distinct-elimination` | Apache Derby DISTINCT Elimination | O(1) |
| `derby-exists-to-join` | Apache Derby EXISTS Subquery Flattening | O(1) |
| `derby-group-by-optimization` | Apache Derby GROUP BY Sort Elimination | O(n) |
| `derby-hash-join` | Apache Derby Hash Join Strategy | O(n+m) |
| `derby-in-list-multi-probe` | Apache Derby IN List Multi-Probe Index Scan | O(k * log n) |
| `derby-index-selection` | Apache Derby Cost-Based Index Selection | O(1) |
| `derby-join-ordering` | Apache Derby Cost-Based Join Ordering | O(n!) |
| `derby-materialized-subquery` | Apache Derby Subquery Materialization | O(n+m) |
| `derby-nested-loop-join` | Apache Derby Index Nested-Loop Join | O(n*k) |
| `derby-not-exists-to-antijoin` | Apache Derby NOT EXISTS to Anti-Join | O(1) |
| `derby-outer-to-inner-join` | Apache Derby Outer-to-Inner Join Conversion | O(1) |
| `derby-predicate-pushdown` | Apache Derby Predicate Pushdown | O(1) |
| `derby-scroll-insensitive-optimization` | Apache Derby Scroll-Insensitive Result Set Optimization | O(n) |
| `derby-sort-avoidance` | Apache Derby Sort Avoidance via Index | O(1) |
| `derby-table-lock-escalation` | Apache Derby Table Lock Escalation | O(1) |
| `derby-transitive-closure` | Apache Derby Transitive Closure of Join Predicates | O(1) |
| `derby-union-all-optimization` | Apache Derby UNION ALL vs UNION Optimization | O(n) |
| `derby-view-flattening` | Apache Derby View Flattening | O(1) |

#### `database-specific/drill/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `drill-dynamic-udf-optimization` | Dynamic UDF Optimization (Drill) | O(n) |
| `drill-late-materialization` | Late Materialization (Drill) | O(n) |
| `drill-schema-discovery-pushdown` | Schema Discovery Pushdown (Drill) | O(n) |
| `drill-schema-versioning` | Schema Versioning (Drill) | O(n) |

#### `database-specific/duckdb/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `duckdb-adaptive-perfect-hash-group` | DuckDB Adaptive Perfect Hash Aggregation | O(n) |
| `duckdb-zonemap-pruning` | DuckDB Zone Map (Min/Max Index) Pruning | O(1) |

#### `database-specific/flink/` (7 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `flink-deduplication-on-changelog` | Deduplication on Changelog Stream (Flink) | O(n) |
| `flink-lookup-join-caching` | Lookup Join Caching (Flink) | O(n) |
| `flink-minibatch-aggregation` | Mini-Batch Aggregation (Flink) | O(n) |
| `flink-retract-optimization` | Retraction Optimization (Flink) | O(n) |
| `flink-stream-table-join-temporal` | Stream-Table Join with Temporal Constraint (Flink) | O(n) |
| `flink-watermark-pushdown` | Watermark Pushdown (Flink) | O(n) |
| `flink-window-aggregation-optimization` | Window Aggregation Optimization (Flink) | O(n) |

#### `database-specific/greenplum/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `greenplum-external-table-pushdown` | External Table Pushdown (Greenplum) | O(n) |
| `greenplum-motion-node-optimization` | Motion Node Optimization (Greenplum) | O(n) |

#### `database-specific/hyper/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `hyper-adaptive-code-generation` | Adaptive Code Generation (HyPer) | O(n) |
| `hyper-morsel-driven-parallelism` | Morsel-Driven Parallelism (HyPer) | O(n) |
| `hyper-vectorized-interpretation` | Vectorized Interpretation (HyPer) | O(n) |

#### `database-specific/impala/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `impala-codegen-disabled-fallback` | Codegen Disabled Fallback (Impala) | O(n) |
| `impala-hdfs-caching` | HDFS Caching (Impala) | O(n) |
| `impala-parquet-predicate-pushdown` | Parquet Predicate Pushdown (Impala) | O(n) |
| `impala-runtime-filter-propagation` | Runtime Filter Propagation (Impala) | O(n) |

#### `database-specific/materialize/` (21 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `materialize-arrangement-sharing` | Materialize Arrangement Sharing | O(n) |
| `materialize-bloom-filter-state-pruning` | Materialize Bloom Filter State Pruning | O(n) build + O(1) per probe |
| `materialize-canonicalize-mfp` | Materialize Canonicalize MapFilterProject | O(n) |
| `materialize-column-knowledge` | Materialize Column Knowledge Propagation | O(n) |
| `materialize-delta-join-planning` | Materialize Delta Join Planning | O(changes * join_selectivity) |
| `materialize-demand-projection` | Materialize Demand-Driven Projection | O(n) |
| `materialize-fusion` | Materialize Operator Fusion | O(n) |
| `materialize-join-implementation` | Materialize Join Implementation Selection | O(n) |
| `materialize-let-motion` | Materialize Let Motion (Common Subexpression Hoisting) | O(n^2) |
| `materialize-literal-lifting` | Materialize Literal Lifting | O(n) |
| `materialize-monotonic-join-optimization` | Materialize Monotonic Join Optimization | O(changes) |
| `materialize-nonnull-requirements` | Materialize NonNull Requirements | O(n) |
| `materialize-predicate-pushdown-through-join` | Materialize Predicate Pushdown Through Join | O(n) |
| `materialize-reduce-elision` | Materialize Reduce Elision | O(n) |
| `materialize-reduce-reduction` | Materialize Reduce Reduction (Hierarchical Aggregation) | O(n) |
| `materialize-sink-projection-pushdown` | Materialize Sink Projection Pushdown | O(n) |
| `materialize-temporal-filter-pushdown` | Materialize Temporal Filter Pushdown | O(n) |
| `materialize-threshold-elision` | Materialize Threshold Elision | O(n) |
| `materialize-time-window-aggregation` | Materialize Time-Window Aggregation | O(changes * log w) |
| `materialize-topk-monotonic` | Materialize Monotonic TopK | O(n log k) |
| `materialize-watermark-propagation` | Materialize Watermark-Based Frontier Propagation | O(operators) |

#### `database-specific/monetdb/` (28 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `monetdb-band-join` | MonetDB Band Join (Theta Join Optimization) | O(n*m) |
| `monetdb-bat-join-ordering` | MonetDB BAT Join Ordering | O(n!) |
| `monetdb-bitwise-bat-operations` | MonetDB Bitwise BAT Operations | O(n/64) |
| `monetdb-bitwise-operations` | MonetDB Bitwise Column Operations | O(n/w) |
| `monetdb-cand-list-intersection` | MonetDB Candidate List Intersection | O(n+m) |
| `monetdb-column-recycling` | MonetDB Intermediate Result Recycling | O(1) |
| `monetdb-columnar-hash-join` | MonetDB Columnar Hash Join | O(n+m) |
| `monetdb-crackers-adaptive-index` | MonetDB Database Cracking (Adaptive Indexing) | O(n) |
| `monetdb-dictionary-compression` | MonetDB Dictionary Encoded Operations | O(n) |
| `monetdb-imprints-scan` | MonetDB Column Imprints Scan | O(n) |
| `monetdb-late-materialization` | MonetDB Late Materialization | O(n) |
| `monetdb-mal-pipeline-optimization` | MonetDB MAL Pipeline Optimization | O(n) |
| `monetdb-merge-join` | MonetDB Ordered Merge Join | O(n+m) |
| `monetdb-mitosis-parallelism` | MonetDB Mitosis Parallel Execution | O(n/p) |
| `monetdb-multi-column-sort-sharing` | MonetDB Multi-Column Sort Sharing | O(n log n) |
| `monetdb-ordered-index-scan` | MonetDB Ordered Index (Persistent Sort) | O(log n + k) |
| `monetdb-partitioned-hash-group` | MonetDB Partitioned Hash Group-By | O(n) |
| `monetdb-projection-pushdown` | MonetDB Columnar Projection Pushdown | O(1) |
| `monetdb-range-select` | MonetDB Vectorized Range Selection | O(n) |
| `monetdb-run-length-encoding` | MonetDB Run-Length Encoded Operations | O(runs) |
| `monetdb-simd-vectorized-selection` | MonetDB SIMD Vectorized Selection | O(n/v) |
| `monetdb-stochastic-cracking` | MonetDB Stochastic Cracking | O(n) |
| `monetdb-strimps-string-filter` | MonetDB Strimps String Filtering | O(n) |
| `monetdb-string-heap-optimization` | MonetDB String Heap Optimization | O(n) |
| `monetdb-tail-ordering` | MonetDB Tail Column Ordering for Joins | O(n log n) |
| `monetdb-window-function-optimization` | MonetDB Window Function Optimization | O(n log n) |
| `monetdb-zone-map-scan-skipping` | MonetDB Zone Map Scan Skipping | O(z + k) |
| `monetdb-zonemap-skipping` | MonetDB Zone Map Data Skipping | O(zones) |

#### `database-specific/mongodb/` (19 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `mongodb-bucket-auto-optimization` | MongoDB $bucketAuto Granularity Optimization | O(n log b) |
| `mongodb-change-stream-optimization` | MongoDB Change Stream Resume Token Optimization | O(1) per change |
| `mongodb-compound-index-selection` | MongoDB Compound Index Selection | O(log n) |
| `mongodb-covered-query` | MongoDB Covered Query Optimization | O(log n) |
| `mongodb-document-projection-pushdown` | MongoDB Document Projection Pushdown | O(n) |
| `mongodb-document-validation` | MongoDB Document Validation Optimization | O(1) |
| `mongodb-geospatial-index` | MongoDB Geospatial Index Optimization | O(log n) |
| `mongodb-graphlookup-optimization` | MongoDB $graphLookup Recursive Traversal Optimization | O(V + E) with index, O(V * N) without |
| `mongodb-hashed-sharding-targeted-query` | MongoDB Hashed Shard Key Targeted Query | O(1) routing + O(log n) shard query |
| `mongodb-index-intersection` | MongoDB Index Intersection | O(n log n) |
| `mongodb-lookup-pipeline-optimization` | MongoDB $lookup Pipeline Optimization | O(n * m) |
| `mongodb-merge-output-optimization` | MongoDB $merge Incremental Output Optimization | O(k) per batch |
| `mongodb-multikey-index-bounds` | MongoDB Multikey Index Bounds Tightening | O(log n) |
| `mongodb-partial-index-selection` | MongoDB Partial Index Selection | O(log n) |
| `mongodb-pipeline-stage-reordering` | MongoDB Aggregation Pipeline Stage Reordering | O(n) |
| `mongodb-sbe-slot-based-execution` | MongoDB Slot-Based Execution Engine (SBE) | O(n) |
| `mongodb-sharded-aggregation` | MongoDB Sharded Aggregation Optimization | O(n/p) |
| `mongodb-text-search-index` | MongoDB Text Search Index Optimization | O(log n) |
| `mongodb-wildcard-index-planning` | MongoDB Wildcard Index Query Planning | O(log n) |

#### `database-specific/mssql/` (20 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `mssql-adaptive-join` | SQL Server Adaptive Join | O(n) |
| `mssql-batch-mode-execution` | SQL Server Batch Mode Execution | O(1) |
| `mssql-batch-mode-on-rowstore` | SQL Server Batch Mode on Rowstore | O(n) |
| `mssql-columnstore-scan` | SQL Server Columnstore Index Scan | O(n) |
| `mssql-hash-match-aggregate` | SQL Server Hash Match Aggregate | O(n) |
| `mssql-in-memory-oltp` | SQL Server In-Memory OLTP (Hekaton) Optimization | O(n) |
| `mssql-indexed-view-matching` | SQL Server Indexed View Matching | O(n) |
| `mssql-interleaved-execution` | SQL Server Interleaved Execution | O(n) |
| `mssql-key-lookup-elimination` | SQL Server Key Lookup Elimination | O(n) |
| `mssql-memory-grant-feedback` | SQL Server Memory Grant Feedback | O(1) |
| `mssql-parameter-sniffing` | SQL Server Parameter Sensitivity Plan | O(n) |
| `mssql-partition-elimination` | SQL Server Partition Elimination | O(1) |
| `mssql-predicate-pushdown-computed` | SQL Server Filter Pushdown to Computed Columns | O(n) |
| `mssql-query-store-forced-plan` | SQL Server Query Store Plan Forcing | O(1) |
| `mssql-seek-predicate-optimization` | SQL Server Index Seek Predicate Optimization | O(n) |
| `mssql-spool-optimization` | SQL Server Spool Optimization | O(n) |
| `mssql-star-join-optimization` | SQL Server Star Join Optimization | O(n) |
| `mssql-subquery-to-apply` | SQL Server Subquery to Apply (Lateral Join) | O(n) |
| `mssql-trivial-plan` | SQL Server Trivial Plan Optimization | O(1) |
| `mssql-window-aggregate-pushdown` | SQL Server Window Function Optimization | O(n log n) |

#### `database-specific/mysql/` (25 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `mysql-batched-key-access` | MySQL Batched Key Access (BKA) | O(n*m) |
| `mysql-condition-fanout-filter` | MySQL Condition Fanout Filter | O(1) |
| `mysql-constant-folding` | MySQL Constant Folding and Propagation | O(1) |
| `mysql-cost-based-subquery` | MySQL Cost-Based Subquery Strategy Selection | O(1) |
| `mysql-covering-index` | MySQL Covering Index Scan | O(n) |
| `mysql-derived-table-merge` | MySQL Derived Table Merge | O(1) |
| `mysql-distinct-optimization` | MySQL DISTINCT Elimination and Optimization | O(n) |
| `mysql-eq-range-index` | MySQL Equality Range Index Access | O(log n) |
| `mysql-exists-to-in` | MySQL EXISTS-to-IN Transformation | O(1) |
| `mysql-group-by-optimization` | MySQL GROUP BY Index Optimization | O(n) |
| `mysql-hash-join` | MySQL Hash Join | O(n+m) |
| `mysql-index-condition-pushdown` | MySQL Index Condition Pushdown (ICP) | O(1) |
| `mysql-index-merge` | MySQL Index Merge Optimization | O(n) |
| `mysql-invisible-index` | MySQL Invisible Index | O(1) |
| `mysql-join-buffer-bnl` | MySQL Block Nested-Loop Join | O(n*m) |
| `mysql-join-elimination` | MySQL Join Elimination | O(1) |
| `mysql-limit-optimization` | MySQL LIMIT Query Optimization | O(1) |
| `mysql-multi-range-read` | MySQL Multi-Range Read (MRR) | O(n log n) |
| `mysql-order-by-optimization` | MySQL ORDER BY Index Optimization | O(1) |
| `mysql-partition-pruning` | MySQL Partition Pruning | O(1) |
| `mysql-predicate-pushdown` | MySQL Predicate Pushdown to Storage Engine | O(1) |
| `mysql-semi-join-strategies` | MySQL Semi-Join Execution Strategies | O(n*m) |
| `mysql-skip-scan` | MySQL Skip Scan Range Access | O(n) |
| `mysql-subquery-materialization` | MySQL Subquery Materialization | O(n+m) |
| `mysql-window-function-optimization` | MySQL Window Function Optimization | O(n) |

#### `database-specific/neo4j/` (17 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `neo4j-bidirectional-bfs` | Neo4j Bidirectional BFS for Shortest Path | O(b^(d/2)) |
| `neo4j-degree-pruning` | Neo4j Degree-Based Pruning for Variable-Length Paths | O(V + E_pruned) |
| `neo4j-eager-aggregation-avoidance` | Neo4j Eager Aggregation Avoidance | O(n) |
| `neo4j-expand-into` | Neo4j Expand Into vs Expand All | O(degree) |
| `neo4j-fulltext-index-query` | Neo4j Full-Text Index Query Optimization | O(log n + k) |
| `neo4j-index-order-by` | Neo4j Index-Backed ORDER BY | O(log n) |
| `neo4j-join-hint-directed-planning` | Neo4j Join Hint Directed Planning | O(n + m) |
| `neo4j-label-scan` | Neo4j Label Scan Optimization | O(n) |
| `neo4j-node-count-from-store` | Neo4j Node Count from Store Statistics | O(1) |
| `neo4j-optional-match-to-anti-semi-apply` | Neo4j OPTIONAL MATCH to AntiSemiApply | O(n * d) |
| `neo4j-pattern-comprehension-optimization` | Neo4j Pattern Comprehension Optimization | O(n * m) |
| `neo4j-property-index-seek` | Neo4j Property Index Seek | O(log n + k) |
| `neo4j-query-plan-cache` | Neo4j Cypher Query Plan Cache | O(1) |
| `neo4j-rel-type-filter` | Neo4j Relationship Type Filtering | O(degree) |
| `neo4j-relationship-index` | Neo4j Relationship Index Usage | O(log R) |
| `neo4j-shortest-path-dijkstra` | Neo4j Shortest Path with Dijkstra | O((V + E) log V) |
| `neo4j-var-length-expansion` | Neo4j Variable-Length Path Expansion Optimization | O(V^depth) |

#### `database-specific/oracle/` (20 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `oracle-adaptive-plans` | Oracle Adaptive Query Plans | O(n) |
| `oracle-batch-table-access` | Oracle Batched Table Access by ROWID | O(n) |
| `oracle-bloom-filter-join` | Oracle Bloom Filter Join Optimization | O(n) |
| `oracle-connect-by-optimization` | Oracle CONNECT BY Optimization | O(n * d) |
| `oracle-group-by-placement` | Oracle Group By Placement | O(n) |
| `oracle-hash-group-by` | Oracle Hash Group By | O(n) |
| `oracle-in-memory-scan` | Oracle In-Memory Column Store Scan | O(n) |
| `oracle-index-fast-full-scan` | Oracle Index Fast Full Scan | O(n) |
| `oracle-join-elimination` | Oracle Join Elimination | O(n) |
| `oracle-join-predicate-pushdown` | Oracle Join Predicate Pushdown | O(n) |
| `oracle-materialized-view-rewrite` | Oracle Materialized View Query Rewrite | O(n) |
| `oracle-or-expansion` | Oracle OR Expansion | O(n) |
| `oracle-parallel-execution` | Oracle Parallel Execution Optimization | O(n) |
| `oracle-partition-pruning` | Oracle Partition Pruning | O(1) |
| `oracle-predicate-move-around` | Oracle Predicate Move-Around | O(n) |
| `oracle-result-cache` | Oracle Result Cache Optimization | O(1) |
| `oracle-star-transformation` | Oracle Star Transformation | O(n) |
| `oracle-subquery-unnesting` | Oracle Subquery Unnesting | O(n) |
| `oracle-table-expansion` | Oracle Table Expansion | O(n) |
| `oracle-view-merging` | Oracle View Merging | O(n) |

#### `database-specific/postgresql/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `postgresql-hash-to-sort-aggregate` | PostgreSQL Hash Aggregate to Sort Aggregate | O(n log n) |
| `postgresql-index-only-scan` | PostgreSQL Index-Only Scan | O(1) |

#### `database-specific/presto/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `presto-cost-based-join-reordering` | Cost-Based Join Reordering (Presto) | O(n!) bounded by heuristics |
| `presto-dynamic-partition-pruning` | Dynamic Partition Pruning (Presto) | O(n) |
| `presto-fragment-result-caching` | Fragment Result Caching (Presto) | O(n) |

#### `database-specific/sqlite/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `sqlite-automatic-index` | SQLite Automatic Index (Transient Index) | O(n log n) |
| `sqlite-covering-index-scan` | SQLite Covering Index Scan | O(1) |

#### `database-specific/tidb/` (19 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `tidb-aggregation-elimination` | TiDB Aggregation Elimination | O(1) |
| `tidb-aggregation-merge` | TiDB Aggregation Merge | O(n) |
| `tidb-aggregation-push-down-join` | Aggregation Push Down Through Join | O(n) |
| `tidb-aggregation-pushdown-decomposable` | TiDB Decomposable Aggregation Pushdown | O(n/p) distributed |
| `tidb-coprocessor-limit-pushdown` | TiDB Coprocessor LIMIT Pushdown | O(n) |
| `tidb-coprocessor-predicate-pushdown` | TiDB Coprocessor Predicate Pushdown | O(n/p) distributed |
| `tidb-coprocessor-projection-pushdown` | TiDB Coprocessor Projection Pushdown | O(n) |
| `tidb-coprocessor-topn-pushdown` | TiDB Coprocessor TOP-N Pushdown | O(n log k) |
| `tidb-derive-topn-from-window` | Derive TopN from Window Function | O(n*log(k)) |
| `tidb-index-merge` | TiDB Index Merge | O(n log n) merge |
| `tidb-index-merge-scan` | Index Merge Scan (Multi-Index OR) | O(n) |
| `tidb-join-reorder-dp` | TiDB Dynamic Programming Join Reordering | O(3^n) for n tables |
| `tidb-outer-join-elimination` | TiDB Outer Join Elimination | O(1) |
| `tidb-partition-pruning` | TiDB Partition Pruning | O(1) partition selection |
| `tidb-predicate-push-down-shard-index` | Shard Index Predicate Prefix Addition | O(n) |
| `tidb-runtime-filter-generation` | Runtime Filter Generation for Hash Joins | O(n) |
| `tidb-semi-join-rewrite` | TiDB Semi-Join Rewrite | O(n+m) |
| `tidb-skew-distinct-agg-rewrite` | Skew-Aware Distinct Aggregation Rewrite | O(n) |
| `tidb-topn-push-down` | TopN and Limit Push Down | O(n*log(k)) |

#### `database-specific/timescaledb/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `timescaledb-chunk-pruning` | Chunk Pruning (TimescaleDB) | O(n) |
| `timescaledb-continuous-aggregates` | Continuous Aggregates (TimescaleDB) | O(n) |
| `timescaledb-time-bucket-aggregation` | Time-Bucket Aggregation (TimescaleDB) | O(n) |

#### `database-specific/trino/` (6 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `trino-adaptive-partitioning` | Adaptive Partitioning (Trino) | O(1) decision, runtime adjustment |
| `trino-adaptive-plan-optimization` | Adaptive Plan Optimization (Trino) | O(n) |
| `trino-connector-pushdown-framework` | Connector Pushdown Framework (Trino) | O(n) |
| `trino-index-join-optimizer` | Index Join Optimizer (Trino) | O(n log m) |
| `trino-limit-pushdown` | Limit Pushdown (Trino) | O(1) transformation |
| `trino-predicate-pushdown-dynamic-filtering` | Predicate Pushdown with Dynamic Filtering (Trino) | O(n) |

#### `database-specific/voltdb/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `voltdb-deterministic-order-optimization` | Deterministic Order Optimization (VoltDB) | O(n) |
| `voltdb-replicated-table-optimization` | Replicated Table Optimization (VoltDB) | O(n) |
| `voltdb-single-partition-optimization` | Single-Partition Optimization (VoltDB) | O(n) |

### distributed/ (58 rules)

Distributed query processing rules.

#### `distributed/colocation/` (6 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `colocation-aware-placement` | Co-location Aware Table Placement | O(n) |
| `multi-join-colocation` | Multi-Way Join Co-location | O(n^2) |
| `partition-key-colocation` | Partition Key Co-Location Join | O(1) |
| `raft-leader-colocation` | Raft Leader Colocation for Joins | O(1) |
| `reference-table-colocation` | Reference Table Co-Location | O(1) |
| `reference-table-join` | Reference Table Join Optimization | O(n) |

#### `distributed/coprocessor-pushdown/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `coprocessor-filter-agg-pushdown` | Coprocessor Filter and Aggregation Pushdown | O(n) |
| `tiflash-mpp-pushdown` | TiFlash MPP Execution Pushdown | O(n) |

#### `distributed/data-movement/` (6 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `minimize-network-transfer` | Minimize Network Transfer via Operator Reordering | O(n^2) |
| `push-filter-below-exchange` | Push Filter Below Exchange | O(n) |
| `push-limit-below-exchange` | Push Limit Below Exchange | O(n) |
| `push-predicate-through-join` | Push Filter Predicate Into Both Join Sides | O(n) |
| `push-project-below-exchange` | Push Projection Below Exchange | O(n) |
| `union-all-pushdown` | Distributed UNION ALL Pushdown | O(n) |

#### `distributed/distributed-joins/` (14 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `asymmetric-repartition-join` | Asymmetric Repartition Join | O(n) |
| `broadcast-join` | Broadcast Join Selection | O(n) |
| `colocated-join` | Co-located Join Elimination | O(n) |
| `hoist-project-from-join` | Hoist Project from Join for Lookup Join Generation | O(n) |
| `inverted-index-join` | Inverted Index Lookup Join | O(n*log(m)) |
| `lookup-join` | Distributed Lookup Join | O(n) |
| `merge-join-generation` | Merge Join Generation from Interesting Orderings | O(n*log(n)) |
| `push-join-into-index-join` | Push Join into Index Join | O(n) |
| `semi-join-reduction` | Distributed Semi-Join Reduction | O(n) |
| `semi-join-to-inner-join` | Semi Join to Inner Join Conversion | O(n) |
| `shuffle-join` | Shuffle (Repartition) Join | O(n) |
| `skew-aware-join` | Skew-Aware Join | O(n log n) |
| `split-disjunction-join` | Split Disjunction of Join Terms | O(n) |
| `zigzag-join` | Zigzag Join on Multiple Indexes | O(n) |

#### `distributed/distributed-sort/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `distributed-topn` | Distributed Top-N | O(n log k) |
| `distributed-topn-push-to-scan` | Distributed TopN Push to Index Scan | O(n*log(k)) |
| `distributed-window-function` | Distributed Window Function Execution | O(n log n) |
| `merge-sort-gather` | Merge-Sort Gather for Distributed Order | O(n log p) |

#### `distributed/distributed-transactions/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `follower-read-optimization` | Follower Read Optimization | O(1) |
| `read-committed-pushdown` | Read Committed Isolation Pushdown | O(1) |

#### `distributed/exchange-placement/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `choose-exchange-type` | Choose Optimal Exchange Type | O(1) |
| `distribute-operator` | Distribute Operator Placement | O(n) |
| `insert-exchange` | Exchange Operator Insertion | O(n) |
| `merge-adjacent-exchanges` | Merge Adjacent Exchanges | O(n) |
| `remove-redundant-exchange` | Remove Redundant Exchange | O(n) |

#### `distributed/locality-optimization/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `locality-optimized-anti-join` | Locality-Optimized Anti Join | O(n) |
| `locality-optimized-lookup-join` | Locality-Optimized Lookup Join | O(n) |
| `locality-optimized-scan` | Locality-Optimized Scan | O(n) |
| `locality-optimized-search-of-join` | Locality-Optimized Search of Lookup Join | O(n) |

#### `distributed/partial-aggregation/` (6 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `eliminate-index-join-inside-group-by` | Eliminate Index Join Inside Group By | O(n) |
| `min-max-to-limit` | Replace Scalar MIN/MAX with LIMIT 1 | O(log(n)) |
| `push-partial-agg-below-exchange` | Push Partial Aggregation Below Exchange | O(n) |
| `streaming-group-by` | Streaming Group By with Interesting Orderings | O(n) |
| `three-phase-distinct-agg` | Three-Phase Distinct Aggregation | O(n) |
| `two-phase-aggregation` | Two-Phase Distributed Aggregation | O(n) |

#### `distributed/partition-pruning/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `dynamic-partition-pruning` | Dynamic Partition Pruning | O(n) |
| `partition-wise-aggregate` | Partition-Wise Aggregation | O(n) |
| `partition-wise-join` | Partition-Wise Join | O(n) |
| `range-partition-pruning` | Range Partition Pruning with Constraint Generation | O(n) |
| `static-partition-pruning` | Static Partition Pruning | O(n) |

#### `distributed/stage-planning/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `distributed-subquery-decorrelation` | Distributed Subquery Decorrelation | O(n*m) |
| `minimize-stage-count` | Minimize Stage Count | O(n) |
| `pipeline-stages` | Pipeline Stage Execution | O(n) |
| `stage-decomposition` | Query Stage Decomposition | O(n) |

### execution-models/ (99 rules)

Execution engine strategies.

#### `execution-models/adaptive/` (11 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-aggregation-switching` | Adaptive Aggregation Strategy Switching | O(input_rows) |
| `adaptive-batch-sizing` | Adaptive Batch Size Tuning | O(1) per batch decision |
| `adaptive-bloom-filter` | Adaptive Runtime Bloom Filter | O(build_rows) build + O(1) per probe |
| `adaptive-index-routing` | Adaptive Index vs. Scan Routing | O(sample_size) per decision |
| `adaptive-join-selection` | Adaptive Runtime Join Selection | O(probe_sample + algorithm_switch) |
| `adaptive-memory-grant` | Adaptive Memory Grant Feedback | O(1) per operator check |
| `adaptive-parallelism-scaling` | Adaptive Parallelism Scaling | O(num_workers) per scaling decision |
| `adaptive-query-reoptimization` | Adaptive Query Re-optimization | O(plan_alternatives * reopt_checks) |
| `adaptive-skew-handling` | Adaptive Data Skew Handling | O(n) detection + O(skewed_partition) redistribution |
| `adaptive-sort-strategy` | Adaptive Sort Strategy Selection | O(n log n) sort + O(1) strategy decision |
| `adaptive-spill-strategy` | Adaptive Spill-to-Disk Strategy | O(n) partition + O(spilled_rows * passes) I/O |

#### `execution-models/column-at-a-time/` (17 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `column-adaptive-execution` | Column-at-a-Time adaptive execution | - |
| `column-aggregate` | Column-at-a-Time Aggregation | - |
| `column-batch-processing` | Column-at-a-Time Batch Processing | O(n) |
| `column-cache-conscious` | Column-at-a-Time cache conscious | - |
| `column-compression` | Column-at-a-Time compression | - |
| `column-compression-aware` | Column-at-a-Time Compression-Aware Processing | O(n / compression_ratio) |
| `column-filter` | Column-at-a-Time Filter with Selection Vectors | - |
| `column-hash-join` | Column-at-a-Time Hash Join | - |
| `column-late-materialization` | Column-at-a-Time Late Materialization | O(n) |
| `column-late-tuple-reconstruction` | Column-at-a-Time Late Tuple Reconstruction | O(k) |
| `column-materialization` | Column-at-a-Time materialization | - |
| `column-projection` | Column-at-a-Time Projection and Expression Evaluation | - |
| `column-scan` | Column-at-a-Time Table Scan (MonetDB X100) | - |
| `column-selection-pushdown` | Column-at-a-Time Selection Vector Pushdown | O(n) |
| `column-string-swar` | Column-at-a-Time SWAR String Processing | O(n * L / 8) |
| `column-vectorized-expression` | Column-at-a-Time Vectorized Expression Evaluation | O(n * expr_depth) |
| `column-vectorized-ops` | Column-at-a-Time vectorized ops | - |

#### `execution-models/differential/` (18 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `differential-arrangement` | Differential Arrangements (Indexed State) | - |
| `differential-changelog` | Differential Changelog Representation | - |
| `differential-collection-operators` | Differential Collection Operators | O(changes) |
| `differential-consolidation` | Differential Collection Consolidation | O(n log n) |
| `differential-delta-query` | Differential Delta Query Processing | - |
| `differential-frontier-advancement` | Differential Frontier Advancement Protocol | O(operators) |
| `differential-incremental-aggregation` | Differential Incremental Aggregation | O(changes per group) |
| `differential-incremental-join` | Differential Incremental Join | O(changes * matches) |
| `differential-incremental-view` | Differential Incremental View Maintenance | - |
| `differential-late-data` | Differential Late-Arriving Data Handling | - |
| `differential-monotonic-topk` | Differential Monotonic Top-K | O(changes * log k) |
| `differential-retraction-handling` | Differential Retraction Handling | O(retractions * fanout) |
| `differential-state-management` | Differential State Management and Compaction | - |
| `differential-stream-aggregate` | Differential Incremental Stream Aggregation | - |
| `differential-stream-join` | Differential Incremental Stream Join | - |
| `differential-time-aware-operators` | Differential Time-Aware Operators | O(changes * log T) |
| `differential-timely-dataflow` | Differential Timely Dataflow Execution | - |
| `differential-watermark` | Differential Watermark and Frontier Tracking | - |

#### `execution-models/experimental/` (8 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-indexing` | Experimental adaptive indexing | - |
| `approximate-query-processing` | Experimental approximate query processing | - |
| `gpu-offloading` | Experimental GPU offloading | - |
| `learned-cost-models` | Experimental learned cost models | - |
| `ml-cardinality-estimation` | Experimental ML cardinality estimation | - |
| `quantum-inspired-optimization` | Experimental quantum inspired optimization | - |
| `query-compilation-jit` | Experimental query compilation JIT | - |
| `rl-join-ordering` | Experimental RL join ordering | - |

#### `execution-models/morsel-driven/` (13 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `morsel-cache-locality` | Morsel-Driven Cache Locality Optimization | O(n) |
| `morsel-driven-adaptive-sizing` | Morsel-Driven Adaptive Morsel Sizing | - |
| `morsel-driven-aggregate` | Morsel-Driven Parallel Aggregation | - |
| `morsel-driven-hash-join` | Morsel-Driven Parallel Hash Join | - |
| `morsel-driven-lock-free` | Morsel-Driven Lock-Free Data Structures | - |
| `morsel-driven-numa-aware` | Morsel-Driven NUMA-Aware Execution | - |
| `morsel-driven-parallelism` | Morsel-Driven Parallel Execution Framework | - |
| `morsel-driven-pipeline` | Morsel-Driven Pipeline Scheduling | - |
| `morsel-driven-scan` | Morsel-Driven Parallel Table Scan | - |
| `morsel-driven-sort` | Morsel-Driven Parallel Sort | - |
| `morsel-driven-work-stealing` | Morsel-Driven Work-Stealing Scheduler | - |
| `morsel-memory-management` | Morsel-Driven Memory Management | O(1) per allocation |
| `morsel-pipeline-breakers` | Morsel-Driven Pipeline Breaker Handling | O(n) per breaker |

#### `execution-models/push-based/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `push-based-adaptive-compilation` | Push-Based Adaptive Compilation Strategy | - |
| `push-based-aggregate` | Push-Based Compiled Aggregation | - |
| `push-based-code-generation` | Push-Based JIT Code Generation | O(plan_size) |
| `push-based-expression-fusion` | Push-Based Expression Fusion | - |
| `push-based-filter` | Push-Based Compiled Filter (Predicate Inlining) | - |
| `push-based-hash-join` | Push-Based Compiled Hash Join | - |
| `push-based-llvm-codegen` | Push-Based LLVM Code Generation | - |
| `push-based-loop-fusion` | Push-Based Loop Fusion (Operator Fusion) | - |
| `push-based-pipeline` | Push-Based Data-Centric Execution Pipeline | O(n) |
| `push-based-scan` | Push-Based Compiled Table Scan | - |

#### `execution-models/vectorized/` (12 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `vectorized-adaptive-batching` | Vectorized Adaptive Batch Sizing | - |
| `vectorized-aggregate` | Vectorized Execution - Batch Aggregation | - |
| `vectorized-compression` | Vectorized Lightweight Column Compression | - |
| `vectorized-expression-eval` | Vectorized SIMD Expression Evaluation | - |
| `vectorized-filter` | Vectorized SIMD Filter | O(n / SIMD_width) |
| `vectorized-hash-join` | Vectorized Execution - Hash Join | - |
| `vectorized-predicate-pushdown` | Vectorized Predicate Pushdown into Scan | - |
| `vectorized-prewhere-chain` | Vectorized Multi-Step PREWHERE Execution | O(n) |
| `vectorized-projection` | Vectorized Execution - Batch Projection | - |
| `vectorized-scan` | Vectorized Execution - Batch Table Scan | - |
| `vectorized-sort` | Vectorized Execution - Cache-Conscious Radix Sort | - |
| `vectorized-topk-filter` | Vectorized Top-K Dynamic Threshold Filter | O(n) |

#### `execution-models/volcano/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `volcano-aggregate` | Volcano Iterator Model - Hash Aggregation | - |
| `volcano-filter` | Volcano Iterator Model - Filter | - |
| `volcano-hash-join` | Volcano Iterator Model - Hash Join | - |
| `volcano-limit` | Volcano Iterator Model - Limit | - |
| `volcano-nested-loop-join` | Volcano Iterator Model - Nested Loop Join | - |
| `volcano-pipeline-breakers` | Volcano Iterator Model - Pipeline Breakers | - |
| `volcano-projection` | Volcano Iterator Model - Projection | - |
| `volcano-scan` | Volcano Iterator Model - Table Scan | - |
| `volcano-sort` | Volcano Iterator Model - External Sort | - |
| `volcano-union` | Volcano Iterator Model - Union | - |

### experimental/ (46 rules)

Experimental and research rules.

#### `experimental/adaptive/` (13 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-aggregation` | Adaptive Aggregation Strategy | O(n) + switching overhead |
| `adaptive-indexing` | Adaptive Indexing (Database Cracking) | O(n) first query, amortized O(log n) |
| `adaptive-join-selection` | Adaptive Join Algorithm Selection | O(n + m) + switching overhead |
| `eddy-operator` | EDDY Adaptive Query Routing | O(n * m) routing decisions |
| `leo-statistics-feedback` | LEO Learning Optimizer Statistics Feedback | O(n) per query + O(1) lookup |
| `mid-query-replanning` | Mid-Query Re-optimization | O(replan_cost) at checkpoints |
| `parametric-query-optimization` | Parametric Query Optimization | O(k * plan_cost) |
| `plan-stability-control` | Plan Stability vs Adaptiveness Control | O(plan_cost) |
| `progressive-optimization` | Progressive Query Optimization (Rio) | O(plan_cost + replan_cost) |
| `query-result-caching` | Query Result Caching and Materialized Subexpression Reuse | O(1) cache lookup |
| `ripple-join` | Ripple Join for Online Aggregation | O(sqrt(n*m)) for confidence interval |
| `runtime-cardinality-feedback` | Runtime Cardinality Feedback Loop | O(1) per execution + O(n) statistics update |
| `runtime-plan-switching` | Runtime Plan Switching with Checkpoint Operators | O(1) per checkpoint |

#### `experimental/approximate/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `approximate-query-processing` | Approximate Query Processing | O(sample_size) instead of O(data_size) |
| `sample-based-join` | Sample-Based Join for Approximate Aggregation | O(s) where s = sample_size |
| `sketches-for-aggregation` | Probabilistic Sketches for Distributed Aggregation | O(n) single-pass |

#### `experimental/compilation/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `query-compilation-jit` | JIT Query Compilation | O(n) compilation, O(n) execution with low constant |
| `vectorized-vs-compiled` | Vectorized vs Compiled Execution Selection | O(n) |

#### `experimental/hardware-accel/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `gpu-offloading` | GPU Query Execution Offloading | O(n/p) where p = GPU cores (thousands) |
| `quantum-inspired-optimization` | Quantum-Inspired Query Optimization | O(sqrt(n)) for search (Grover's), variable for optimization |

#### `experimental/ml-guided/` (9 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `learned-cardinality` | Learned Cardinality Estimation | O(1) inference per subplan |
| `learned-cost-calibration` | Learned Cost Model Calibration | O(1) per operator |
| `learned-cost-models` | Learned Cost Models | O(1) inference per plan |
| `learned-join-ordering` | Learned Join Ordering (Neo/Bao) | O(n^2) inference per query |
| `learned-query-scheduling` | Learned Query Scheduling and Resource Allocation | O(q log q) for q concurrent queries |
| `ml-cardinality-estimation` | ML-Based Cardinality Estimation | O(1) inference per subplan |
| `plan-hint-generation` | ML-Based Plan Hint Generation | O(h * plan_cost) for h hints |
| `rl-join-ordering` | Reinforcement Learning for Join Ordering | O(n) per join ordering decision (n = tables) |
| `workload-aware-indexing` | ML-Guided Workload-Aware Index Selection | O(w * c) for w queries, c candidate indexes |

#### `experimental/semantic/` (7 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `commutativity-aware-rewriting` | Commutativity-Aware Algebraic Rewriting | O(n^2) rule applications |
| `constraint-based-rewriting` | Constraint-Based Semantic Rewriting | O(n * |constraints|) |
| `egg-extraction-strategies` | E-graph Extraction Strategies for Query Optimization | O(n * k) per extraction |
| `equality-saturation` | Equality Saturation Query Rewriting | O(n log n) per iteration |
| `functional-dependency-rewrite` | Functional Dependency-Based Rewriting | O(n * |FDs|) |
| `hottsql-proof-rewrite` | HoTTSQL Proof-Based Query Rewriting | O(proof_search) |
| `semijoin-reduction` | Semi-Join Reduction Programs | O(n * k) reductions |

#### `experimental/wcoj/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `delta-wcoj` | Delta WCOJ for Incremental Maintenance | O(delta_N^($\rho$*)) |
| `factorized-join` | Factorized Join Representation | O(N^(fhtw)) |
| `free-join` | Free Join (Worst-Case Optimal Join) | O(N^($\rho$*)) |
| `generic-join` | Generic Join (Ngo-Porat-Re-Rudra) | O(N^($\rho$*)) |
| `honeycomb-join` | HoneyComb Distributed WCOJ | O(N^($\rho$*) / p) |
| `leapfrog-triejoin` | LeapFrog TrieJoin | O(N^($\rho$*)) |
| `level-headed-join` | LevelHeaded Join Algorithm | O(N^($\rho$*) * w) |
| `wcoj-clique-detection` | WCOJ for Clique Detection Patterns | O(N^(k/2)) |
| `wcoj-star-pattern` | WCOJ for Star Join Patterns | O(N * sqrt(N)) |
| `wcoj-to-binary-fallback` | WCOJ to Binary Join Fallback | O(min(N^$\rho$*, binary_plan)) |

### hardware/ (21 rules)

Hardware-aware optimization rules.

#### `hardware/accelerator/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `cache-conscious-partitioning` | Cache-Conscious Radix Partitioning | O(n * passes) |
| `heterogeneous-operator-placement` | Heterogeneous Operator Placement | plan-level |
| `numa-aware-partitioning` | NUMA-Aware Data Partitioning | O(n/s) |
| `prefetch-aware-join` | Software Prefetch-Aware Hash Join | O(n+m) |
| `simd-vectorized-scan` | SIMD-Vectorized Scan and Filter | O(n/w) |

#### `hardware/data-placement/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `columnar-conversion` | Row-to-Columnar Conversion for Device Processing | O(n*c) |
| `device-memory-caching` | Device Memory Caching and Reuse | plan-level |
| `host-to-device-transfer` | Minimize Host-to-Device Data Transfer | plan-level |
| `unified-memory-management` | Unified Memory Management for CPU-GPU | plan-level |

#### `hardware/fpga/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `fpga-compression-scan` | FPGA Near-Storage Decompression Scan | O(n) at storage bandwidth |
| `fpga-hash-join` | FPGA Pipelined Hash Join | O(n+m) pipelined |
| `fpga-regex-filter` | FPGA Hardware Regex Filter | O(n*k) at wire speed |
| `fpga-stream-filter` | FPGA Streaming Filter | O(n) at wire speed |

#### `hardware/gpu/` (8 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `gpu-aggregation` | GPU Parallel Aggregation | O(n/p + g) |
| `gpu-distinct-aggregation` | GPU Two-Phase Distinct Aggregation | O(n/p + d) |
| `gpu-hash-join` | GPU Hash Join | O((n+m)/p) |
| `gpu-parallel-scan` | GPU Parallel Table Scan | O(n/p) |
| `gpu-predicate-evaluation` | GPU SIMT Predicate Evaluation | O(n/p) |
| `gpu-sort` | GPU Parallel Sort | O(n log n / p) |
| `gpu-string-operations` | GPU Accelerated String Operations | O(n*k/p) |
| `gpu-window-function` | GPU Parallel Window Function | O(n/p + w*log(p)) |

### logical/ (209 rules)

Logical query rewrite rules.

#### `logical/aggregate-pushdown/` (22 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `aggregate-distinct-optimization` | Aggregate DISTINCT Optimization | O(1) |
| `aggregate-over-aggregate-fusion` | Aggregate Over Aggregate Fusion | O(1) |
| `aggregate-selectivity-estimation` | Aggregate Selectivity Estimation | O(1) |
| `aggregate-through-union` | Aggregate Through Union | O(1) |
| `aggregate-with-constant-elimination` | Aggregate with Constant Elimination | O(1) |
| `calcite-aggregate-case-to-filter` | Calcite AggregateCaseToFilterRule | O(n) |
| `calcite-aggregate-extract-project` | Calcite AggregateExtractProjectRule | O(n) |
| `calcite-aggregate-filter-to-case` | Calcite AggregateFilterToCaseRule | O(n) |
| `calcite-aggregate-filter-to-filtered-aggregate` | Calcite AggregateFilterToFilteredAggregateRule | O(n) |
| `calcite-aggregate-filter-transpose` | Calcite AggregateFilterTransposeRule | O(n) |
| `calcite-aggregate-grouping-sets-to-union` | Calcite AggregateGroupingSetsToUnionRule | O(k*n) |
| `calcite-aggregate-min-max-to-limit` | Calcite AggregateMinMaxToLimitRule | O(1) |
| `calcite-aggregate-project-pull-up-constants` | Calcite AggregateProjectPullUpConstantsRule | O(n) |
| `calcite-aggregate-union-aggregate` | Calcite AggregateUnionAggregateRule | O(n) |
| `calcite-project-aggregate-merge` | Calcite ProjectAggregateMergeRule | O(n) |
| `count-star-optimization` | COUNT(*) Optimization | O(1) |
| `distinct-to-group-by` | DISTINCT to GROUP BY | O(1) |
| `eager-aggregation` | Eager Aggregation (Yan & Larson) | O(n) |
| `group-by-pushdown-through-join` | Group-By Pushdown Through Join | O(1) |
| `having-to-filter-separation` | HAVING to Filter Separation | O(1) |
| `min-max-index-scan` | MIN/MAX Index Scan | O(1) |
| `partial-aggregation-insertion` | Partial Aggregation Insertion | O(1) |

#### `logical/cte-optimization/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `cte-inlining` | CTE Inlining | O(1) |
| `cte-materialization` | CTE Materialization | O(n) |
| `cte-merge-duplicate` | CTE Merge Duplicate Definitions | O(n) |
| `cte-predicate-pushdown` | CTE Predicate Pushdown | O(1) |
| `cte-projection-pushdown` | CTE Projection Pushdown | O(1) |

#### `logical/distinct-elimination/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `distinct-after-group-by` | Distinct After GROUP BY Elimination | O(1) |
| `distinct-filter-reorder` | Distinct Filter Reorder | O(1) |
| `distinct-on-unique-key` | Distinct on Unique Key Elimination | O(1) |
| `distinct-pushdown-through-union` | Distinct Pushdown Through UNION | O(1) |
| `distinct-to-limit-one` | Distinct to Limit One | O(1) |

#### `logical/expression-simplification/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `arithmetic-simplification` | Arithmetic Expression Simplification | - |
| `boolean-simplification` | Boolean Expression Simplification | - |
| `calcite-filter-remove-is-not-distinct-from` | Calcite FilterRemoveIsNotDistinctFromRule | O(n) |
| `calcite-project-over-sum-to-sum0` | Calcite ProjectOverSumToSum0Rule | O(n) |
| `calcite-reduce-decimals` | Calcite ReduceDecimalsRule | O(n) |
| `codd-relational-completeness` | Codd's Relational Algebra Equivalences | O(1) |
| `common-subexpression-elimination` | Common Subexpression Elimination | - |
| `constant-folding` | Constant Folding | - |
| `null-propagation` | NULL Propagation Simplification | - |
| `starburst-contradiction-detection` | Starburst Contradiction Detection and Unsatisfiable Query Elimination | O(p^2) |

#### `logical/function-optimization/` (58 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `aggregate-function-decomposition` | Aggregate Function Decomposition | O(1) |
| `aggregate-function-simplification` | Simplify Aggregate Function Expressions | O(1) |
| `array-function-optimization` | Array Function Optimization | O(1) plan transform |
| `between-to-range` | BETWEEN to Range Predicates | O(1) |
| `case-simplification` | CASE Expression Simplification | O(n) |
| `cast-elimination` | Cast Elimination | O(1) |
| `cast-elimination-for-index` | Remove Casts That Prevent Index Usage | O(1) |
| `coalesce-simplification` | COALESCE Simplification | O(1) |
| `collation-aware-index-use` | Match Collation in String Index Comparisons | O(1) |
| `comparison-normalization` | Comparison Normalization | O(1) |
| `constant-fold-aggregate` | Constant Fold Aggregate on Constants | O(1) |
| `constant-fold-cast` | Constant Fold CAST Operations | O(1) |
| `constant-fold-comparison` | Constant Fold Comparison Operators | O(1) |
| `constant-fold-conditional` | Constant Fold Conditional Expressions | O(1) |
| `constant-fold-datetime` | Constant Fold DateTime Functions | O(1) |
| `constant-fold-logical` | Constant Fold Logical Operators | O(1) |
| `constant-fold-math` | Constant Fold Math Functions | O(1) |
| `constant-fold-nested` | Constant Fold Nested Function Calls | O(d) |
| `constant-fold-null` | Constant Fold NULL Expressions | O(1) |
| `constant-fold-string` | Constant Fold String Functions | O(1) |
| `constant-function-folding` | Constant Function Folding | O(1) |
| `date-function-optimization` | Date/Time Function Optimization | O(1) |
| `deterministic-function-caching` | Cache Repeated Deterministic Function Calls | O(n) |
| `deterministic-function-dedup` | Deterministic Function Deduplication | O(1) plan transform |
| `expensive-function-above-join` | Expensive Function Above Join | O(1) plan transform |
| `expensive-function-caching` | Expensive Function Result Caching | O(1) plan transform |
| `expensive-function-late-eval` | Delay Expensive Function Evaluation | O(n) |
| `expensive-predicate-ordering` | Expensive Predicate Ordering | O(k log k) for k predicates |
| `expression-index-matching` | Expression Index Matching | O(n) |
| `expression-index-rewrite` | Rewrite Expressions to Match Expression Indexes | O(n) |
| `function-based-index-scan` | Use Function-Based Index for Matching Predicates | O(1) |
| `function-cost-reordering` | Function Evaluation Cost Reordering | O(k log k) for k expressions |
| `function-filter-pushdown` | Push Function Predicates Below Joins | O(n) |
| `function-index-matching-composite` | Function-Based Expression Index Matching | O(1) plan transform |
| `function-inlining` | Function Inlining | O(n) |
| `function-inverse-transform` | Apply Function Inverse to Enable Index Use | O(1) |
| `function-projection-pruning` | Prune Unused Function Calls from Projections | O(n) |
| `function-pushdown-past-aggregate` | Function Pushdown Past Aggregate | O(1) plan transform |
| `function-pushdown-past-join` | Function Pushdown Past Join | O(1) plan transform |
| `geospatial-function-optimization` | Geospatial Function Optimization | O(1) plan transform |
| `greatest-least-optimization` | GREATEST/LEAST Optimization | O(n) |
| `if-null-to-coalesce` | IFNULL/NVL to COALESCE Normalization | O(1) |
| `implicit-cast-index-match` | Resolve Implicit Casts to Enable Index Use | O(1) |
| `in-list-optimization` | IN List Optimization | O(n) |
| `is-null-optimization` | IS NULL/IS NOT NULL Optimization | O(1) |
| `json-function-optimization` | JSON Function Optimization | O(1) plan transform |
| `like-to-range` | LIKE to Range Conversion | O(1) |
| `not-pushdown` | NOT Pushdown (De Morgan's Laws) | O(1) |
| `nullif-simplification` | NULLIF Simplification | O(1) |
| `partial-index-predicate-match` | Match Function Predicates with Partial Indexes | O(n) |
| `predicate-implication` | Predicate Implication Detection | O(n^2) |
| `pure-function-cse` | Pure Function Common Subexpression Elimination | O(n) expression tree walk |
| `sargable-function-rewrite` | SARGable Function Rewrite | O(1) |
| `string-function-optimization` | String Function Optimization | O(1) |
| `timezone-aware-index-use` | Match Timezone Conversions for Index Use | O(1) |
| `type-inference-optimization` | Type Inference Optimization | O(n) |
| `volatile-function-barrier` | Volatile Function Barrier | O(1) |
| `window-function-optimization` | Window Function Optimization | O(n) |

#### `logical/join-elimination/` (19 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `anti-join-simplification` | Anti-Join Simplification | O(1) |
| `anti-join-to-not-exists` | Anti-Join to NOT EXISTS | O(1) |
| `calcite-aggregate-join-join-remove` | Calcite AggregateJoinJoinRemoveRule | O(n) |
| `calcite-aggregate-join-remove` | Calcite AggregateJoinRemoveRule | O(n) |
| `calcite-project-join-join-remove` | Calcite ProjectJoinJoinRemoveRule | O(n) |
| `calcite-project-join-remove` | Calcite ProjectJoinRemoveRule | O(n) |
| `cross-join-elimination` | Cross Join Elimination | O(1) |
| `degenerate-join-to-filter` | Degenerate Join to Filter | O(1) |
| `foreign-key-join-elimination` | Foreign Key Join Elimination | O(1) |
| `inner-join-identity-elimination` | Inner Join Identity Elimination | O(1) |
| `key-propagation-for-join-elimination` | Key Propagation for Join Elimination | O(n) |
| `left-join-elimination` | Left Join Elimination | O(1) |
| `left-join-null-rejection` | LEFT JOIN Null Rejection | O(1) |
| `outer-join-to-filter` | Outer Join to Filter | O(1) |
| `redundant-join-elimination` | Redundant Join Elimination | O(1) |
| `self-join-elimination` | Self-Join Elimination | O(1) |
| `semi-join-to-existence-check` | Semi-Join to Existence Check | O(1) |
| `starburst-unique-key-join-elimination` | Unique Key Join Elimination | O(n) |
| `unique-key-join-elimination` | Unique Key Join Elimination | O(1) |

#### `logical/join-reordering/` (9 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-dphyp-join-reorder` | Calcite DphypJoinReorderRule | O(2^n) |
| `calcite-multi-join-optimize-bushy` | Calcite MultiJoinOptimizeBushyRule | O(n^2) |
| `cartesian-to-join` | Cartesian Product to Join Conversion | - |
| `join-associativity` | Join Associativity | - |
| `join-commutativity` | Join Commutativity | - |
| `left-deep-to-bushy` | Left-Deep to Bushy Join Tree | - |
| `outer-join-to-inner` | Outer Join to Inner Join Simplification | - |
| `system-r-dynamic-programming` | System R Dynamic Programming Join Ordering | O(n * 2^n) |
| `system-r-left-deep-enumeration` | System R Left-Deep Join Tree Enumeration | O(n * 2^n) |

#### `logical/limit-pushdown/` (12 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-sort-join-copy` | Calcite SortJoinCopyRule | O(n log n) |
| `calcite-sort-merge` | Calcite SortMergeRule | O(n) |
| `calcite-sort-remove-constant-keys` | Calcite SortRemoveConstantKeysRule | O(n) |
| `calcite-sort-remove-redundant` | Calcite SortRemoveRedundantRule | O(1) |
| `limit-before-order-by` | LIMIT Before ORDER BY | O(1) |
| `limit-one-to-exists` | LIMIT 1 to EXISTS | O(1) |
| `limit-through-aggregate` | LIMIT Through Aggregate | O(1) |
| `limit-through-projection` | LIMIT Through Projection | O(1) |
| `limit-through-union-all` | LIMIT Through UNION ALL | O(1) |
| `limit-with-top-k-join` | LIMIT with Top-K Join | O(1) |
| `offset-zero-elimination` | OFFSET Zero Elimination | O(1) |
| `sort-limit-fusion` | Sort-Limit Fusion | O(1) |

#### `logical/predicate-pushdown/` (17 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-filter-correlate` | Calcite FilterCorrelateRule | O(n) |
| `calcite-filter-join` | Calcite FilterJoinRule | O(n) |
| `calcite-filter-sort-transpose` | Calcite FilterSortTransposeRule | O(n) |
| `calcite-filter-table-function-transpose` | Calcite FilterTableFunctionTransposeRule | O(n) |
| `calcite-filter-table-scan` | Calcite FilterTableScanRule | O(1) |
| `calcite-join-extract-filter` | Calcite JoinExtractFilterRule | O(n) |
| `calcite-join-push-expressions` | Calcite JoinPushExpressionsRule | O(n) |
| `calcite-join-push-transitive-predicates` | Calcite JoinPushTransitivePredicatesRule | O(n) |
| `filter-into-join-condition` | Filter Absorption Into Join Condition | - |
| `filter-merge` | Filter Merge (Cascading Selections) | - |
| `filter-through-join` | Filter Pushdown Through Join | - |
| `filter-through-project` | Filter Pushdown Through Projection | - |
| `filter-through-union` | Filter Pushdown Through Union | - |
| `predicate-transitive-closure` | Predicate Transitive Closure | O(e * p) |
| `starburst-constraint-propagation` | Starburst Constraint-Based Predicate Propagation | O(c * p) |
| `starburst-referential-integrity-rewrite` | Starburst Referential Integrity Rewrite | O(n) |
| `starburst-semantic-optimization` | Starburst Semantic Query Optimization | O(n) |

#### `logical/projection-pushdown/` (7 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-join-project-transpose` | Calcite JoinProjectTransposeRule | O(n) |
| `calcite-project-filter-transpose` | Calcite ProjectFilterTransposeRule | O(n) |
| `calcite-project-join-transpose` | Calcite ProjectJoinTransposeRule | O(n) |
| `calcite-project-table-scan` | Calcite ProjectTableScanRule | O(1) |
| `column-pruning` | Column Pruning (Dead Column Elimination) | - |
| `project-merge` | Projection Merge | - |
| `project-through-join` | Projection Pushdown Through Join | - |

#### `logical/semantic-rewriting/` (8 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-calc-remove` | Calcite CalcRemoveRule | O(1) |
| `calcite-calc-split` | Calcite CalcSplitRule | O(n) |
| `calcite-filter-calc-merge` | Calcite FilterCalcMergeRule | O(n) |
| `calcite-full-to-left-and-right-join` | Calcite FullToLeftAndRightJoinRule | O(n) |
| `calcite-join-to-correlate` | Calcite JoinToCorrelateRule | O(n*m) |
| `calcite-mark-to-semi-or-anti-join` | Calcite MarkToSemiOrAntiJoinRule | O(n) |
| `calcite-project-calc-merge` | Calcite ProjectCalcMergeRule | O(n) |
| `calcite-sample-to-filter` | Calcite SampleToFilterRule | O(n) |

#### `logical/set-operations/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-intersect-reorder` | Calcite IntersectReorderRule | O(k log k) |
| `calcite-intersect-to-exists` | Calcite IntersectToExistsRule | O(n) |
| `calcite-minus-to-anti-join` | Calcite MinusToAntiJoinRule | O(n) |
| `calcite-minus-to-distinct` | Calcite MinusToDistinctRule | O(n) |
| `calcite-set-op-to-filter` | Calcite SetOpToFilterRule | O(n) |
| `calcite-union-to-distinct` | Calcite UnionToDistinctRule | O(n) |
| `calcite-union-to-values` | Calcite UnionToValuesRule | O(1) |
| `except-to-anti-join` | EXCEPT to Anti-Join | O(1) |
| `intersect-to-join` | Intersect to Semi-Join Conversion | - |
| `union-merge` | Union Merge (Flatten Nested Unions) | - |

#### `logical/sideways-information-passing/` (3 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `magic-sets-adorned-predicates` | Magic Sets Adorned Predicate Generation | O(r * 2^a) |
| `magic-sets-sideways-passing` | Sideways Information Passing Strategy (SIPS) | O(n!) |
| `magic-sets-supplementary-predicates` | Magic Sets Supplementary Predicate Creation | O(r) |

#### `logical/subquery-unnesting/` (17 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `all-to-aggregation` | ALL Subquery to Aggregation | O(1) |
| `apply-to-join` | Apply to Join | O(n) |
| `calcite-unnest-decorrelate` | Calcite UnnestDecorrelateRule | O(n) |
| `correlated-any-to-semi-join` | Correlated ANY to Semi-Join | O(1) |
| `correlated-subquery-decorrelation` | Correlated Subquery Decorrelation | O(n) |
| `exists-to-semi-join` | EXISTS to Semi-Join | O(1) |
| `in-subquery-to-semi-join` | IN Subquery to Semi-Join | O(1) |
| `lateral-join-decorrelation` | Lateral Join Decorrelation | O(n) |
| `magic-sets-rewriting` | Magic Sets Rewriting | O(n) |
| `max-1-row-subquery-check` | Max-1-Row Subquery Check | O(1) |
| `not-exists-to-anti-join` | NOT EXISTS to Anti-Join | O(1) |
| `not-in-to-anti-join` | NOT IN to Anti-Join | O(1) |
| `scalar-subquery-to-join` | Scalar Subquery to Join | O(1) |
| `subquery-deduplication` | Subquery Deduplication | O(n$^2$) |
| `subquery-hoisting` | Subquery Hoisting | O(n) |
| `subquery-with-aggregation-unnesting` | Subquery with Aggregation Unnesting | O(n) |
| `uncorrelated-subquery-to-join` | Uncorrelated Subquery to Join | O(1) |

#### `logical/view-rewriting/` (2 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `starburst-query-graph-model` | Starburst Query Graph Model (QGM) | O(n) |
| `starburst-view-merging` | Starburst View Merging and Unfolding | O(n) |

#### `logical/window-pushdown/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `window-function-pushdown` | Window Function Filter Pushdown | O(1) |
| `window-merge` | Window Function Merge | O(1) |
| `window-partition-elimination` | Window Partition Elimination | O(1) |
| `window-projection-pushdown` | Window Projection Pushdown | O(1) |
| `window-to-aggregate` | Window to Aggregate Conversion | O(1) |

### multi-model/ (30 rules)

Multi-model (document, graph, timeseries).

#### `multi-model/document/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `array-unwind-pushdown` | Array Unwind Pushdown | - |
| `change-stream-filter-pushdown` | Change Stream Filter Pushdown | - |
| `compound-index-selection` | Compound Index Selection for Nested Fields | - |
| `group-push-accumulator` | Group Push Accumulator Optimization | - |
| `lookup-to-embedded` | Lookup to Embedded Document Access | - |
| `nested-predicate-pushdown` | Nested Predicate Pushdown | - |
| `pipeline-coalescence` | Aggregation Pipeline Coalescence | - |
| `projection-to-covered-query` | Projection to Covered Query | - |
| `schema-inference-pushdown` | Schema Inference Pushdown | - |
| `shard-key-targeted-query` | Shard Key Targeted Query | - |

#### `multi-model/graph/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `bidirectional-search` | Bidirectional Search Optimization | - |
| `degree-aware-join-ordering` | Degree-Aware Join Ordering | - |
| `expand-into` | Expand Into Optimization | - |
| `join-to-traversal` | Join to Graph Traversal Conversion | - |
| `label-scan-pushdown` | Label Scan Pushdown | - |
| `path-materialization` | Path Materialization for Repeated Traversals | - |
| `pattern-decomposition` | Graph Pattern Decomposition | - |
| `predicate-pushdown-through-traversal` | Predicate Pushdown Through Traversal | - |
| `subgraph-isomorphism-pruning` | Subgraph Isomorphism Pruning | - |
| `vertex-centric-index` | Vertex-Centric Index Selection | - |

#### `multi-model/timeseries/` (10 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `aligned-aggregation-merge` | Aligned Aggregation Merge | - |
| `delta-encoding-scan` | Delta Encoding Aware Scan | - |
| `downsampling-pushdown` | Downsampling Pushdown | - |
| `gap-fill-pushdown` | Gap Fill Pushdown | - |
| `last-point-optimization` | Last Point Query Optimization | - |
| `retention-policy-pruning` | Retention Policy Pruning | - |
| `segment-merge-elimination` | Segment Merge Elimination | - |
| `tag-index-scan` | Tag Index Scan for Series Filtering | - |
| `time-range-pruning` | Time Range Pruning | - |
| `window-function-pushdown` | Window Function Pushdown for Time Series | - |

### physical/ (108 rules)

Physical operator selection rules.

#### `physical/access-path-selection/` (4 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `system-r-access-path-selection` | System R Access Path Selection | O(p * i) |
| `system-r-interesting-sort-orders` | System R Interesting Sort Orders | O(n log n) |
| `system-r-nested-loop-selection` | System R Nested-Loop Join Selection | O(n * m) |
| `system-r-sort-merge-selection` | System R Sort-Merge Join Selection | O(n log n + m log m) |

#### `physical/aggregation-strategies/` (16 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-aggregation` | Adaptive Aggregation | O(n) |
| `aggregation-pushdown` | Aggregation Pushdown Below Join | O(n) |
| `columnar-aggregation` | Columnar Aggregation with Column-at-a-Time Processing | O(n) |
| `distinct-aggregation-strategy` | Distinct Aggregation Strategy | O(n) |
| `grouping-sets-expansion` | Grouping Sets Expansion | O(n * k) |
| `hash-aggregation` | Hash Aggregation | O(n) |
| `hybrid-aggregation` | Hybrid Aggregation | O(n) |
| `ordered-aggregation` | Ordered Aggregation (Aggregation-in-Order) | O(n) |
| `partial-aggregation-cost-model` | Partial Aggregation Cost Model (When to Pre-Aggregate) | O(n) |
| `preaggregation` | Preaggregation | O(n) |
| `sort-aggregation` | Sort Aggregation | O(n log n) |
| `sort-based-aggregation` | Sort-Based Aggregation | O(n log n) |
| `streaming-aggregation` | Streaming Aggregation | O(n) |
| `three-phase-aggregation` | Three-Phase Aggregation | O(n) |
| `two-phase-aggregation` | Two-Phase Aggregation | O(n) |
| `vectorized-aggregation` | Vectorized Aggregation | O(n) |

#### `physical/index-selection/` (36 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-index-selection` | Adaptive Index Selection with Runtime Feedback | O(log n + k) |
| `bitmap-index-combining` | Bitmap Index Combining with AND/OR | O(n/64) |
| `bitmap-index-selection` | Bitmap Index Selection | O(n) |
| `brin-index-for-sequential` | BRIN Index for Sequential/Temporal Data | O(1) per range + O(k) pages |
| `clustered-index-for-range` | Clustered Index for Range Predicates | O(log n + k) |
| `clustered-index-selection` | Clustered Index Selection | O(log n) |
| `columnstore-for-aggregation` | Columnstore Index for Aggregation Queries | O(n/compression) |
| `composite-index-column-order` | Composite Index Column Order Selection | O(log n + k) |
| `covering-index-optimization` | Covering Index Optimization | O(log n + k) |
| `covering-index-selection` | Covering Index Selection | O(log n + k) |
| `dbmin-index-selection` | Index Selection for Conjunctive Queries | O(n) |
| `expression-index-selection` | Expression Index Selection | O(log n) |
| `filtered-index-matching` | Filtered (Partial) Index Matching | O(log n + k) |
| `full-text-index-for-like` | Full-Text Index for LIKE Patterns | O(log n + k) |
| `gin-index-for-arrays` | GIN Index for Array Operations | O(log n + k) |
| `gist-index-for-spatial` | GiST Index for Spatial and Range Types | O(log n + k) |
| `hash-index-for-equality` | Hash Index for Equality Predicates | O(1) |
| `hash-index-selection` | Hash Index Selection for Equality Lookups | O(1) |
| `index-cost-comparison` | Index vs Sequential Scan Cost Comparison | O(n) or O(log n + k) |
| `index-intersection` | Index Intersection | O(log n + log m) |
| `index-intersection-vs-union` | Index Intersection vs Union Selection | O(n log n) |
| `index-join` | Index Join | O(n * log m) |
| `index-merge-intersection` | Index Merge Intersection | O(n1 + n2) for merge, O(k) for result |
| `index-only-scan` | Index-Only Scan | O(log n + k) |
| `index-only-scan-preference` | Index-Only Scan Preference | O(log n + k) |
| `index-scan` | Index Scan | O(log n + k) |
| `index-skip-scan` | Index Skip Scan | O(d * log n) |
| `index-union` | Index Union | O(log n + log m) |
| `loose-index-scan` | Loose Index Scan | O(g * log n) |
| `multi-column-index-selection` | Multi-Column Index Selection | O(log n) |
| `partial-index-selection` | Partial Index Selection | O(log n) |
| `range-index-selection` | Range Index Selection for Ordered Access | O(log n + k) |
| `reverse-index-scan` | Reverse Index Scan | O(log n + k) |
| `sparse-index-scan` | Sparse Index Scan (Granule-Level Pruning) | O(log n + k) |
| `spatial-index-for-geometry` | Spatial Index for Geometry Queries | O(log n + k) |
| `unique-index-for-equality` | Unique Index for Equality Lookups | O(log n) |

#### `physical/join-algorithms/` (18 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `adaptive-hash-join` | Adaptive Hash Join | O(n + m) |
| `adaptive-join-algorithm` | Adaptive Join Algorithm | O(n + m) to O(n * m) |
| `band-join` | Band Join | O(n log n + n * w) |
| `block-nested-loop-join` | Block Nested Loop Join | O(n * m / b) |
| `broadcast-hash-join` | Broadcast Hash Join | O(n + m) |
| `calcite-semi-join-join-transpose` | Calcite SemiJoinJoinTransposeRule | O(n) |
| `calcite-semi-join-project-transpose` | Calcite SemiJoinProjectTransposeRule | O(n) |
| `grace-hash-join` | Grace Hash Join | O(n + m) |
| `hash-join` | Hash Join | O(n + m) |
| `hash-join-with-bloom-filter` | Hash Join with Bloom Filter | O(n + m) |
| `hybrid-hash-join` | Hybrid Hash Join | O(n + m) |
| `index-nested-loop-join` | Index Nested Loop Join | O(n * log m) |
| `nested-loop-join` | Nested Loop Join | O(n * m) |
| `radix-hash-join` | Radix Hash Join | O(n + m) |
| `shapiro-symmetric-hash-join` | Shapiro Symmetric Hash Join | O(n + m) |
| `shuffle-hash-join` | Shuffle Hash Join | O(n + m) |
| `sort-merge-join` | Sort-Merge Join | O(n log n + m log m) |
| `zigzag-join` | Zigzag Join | O(k * log n) |

#### `physical/materialization/` (13 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `calcite-materialized-view-filter-scan` | Calcite MaterializedViewFilterScanRule | O(1) |
| `common-subexpression-materialization` | Common Subexpression Materialization | O(n) |
| `cte-materialization-strategy` | CTE Materialization Strategy Selection | O(n) |
| `eager-materialization` | Eager Materialization | O(n) |
| `in-memory-materialization` | In-Memory Materialization | O(n) |
| `incremental-view-maintenance` | Incremental View Maintenance | O(delta) |
| `lazy-materialization` | Lazy Materialization | O(n) |
| `materialized-view-rewrite` | Materialized View Rewrite | O(1) |
| `pipeline-breaker-analysis` | Pipeline Breaker Analysis and Minimization | O(n) |
| `result-caching` | Result Caching | O(1) |
| `temp-table-materialization` | Temp Table Materialization | O(n) |
| `volcano-interesting-orders` | Volcano Interesting Orders | O(n log n) |
| `work-table-spooling` | Work Table Spooling | O(n) |

#### `physical/optimizer-framework/` (5 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `cascades-memo-structure` | Cascades Memo Structure and Group Optimization | O(n) per lookup, O(n * 2^n) total |
| `volcano-branch-and-bound` | Volcano Branch-and-Bound Pruning | O(n * 2^n) worst case, much better in practice |
| `volcano-enforcer-placement` | Volcano Enforcer Placement | O(n) |
| `volcano-logical-to-physical` | Volcano Logical-to-Physical Transformation | O(n) |
| `volcano-transformation-rules` | Volcano Logical Transformation Rules | O(n) |

#### `physical/parallelization/` (16 rules)

| Rule ID | Name | Complexity |
|---------|------|------------|
| `bushy-parallelism` | Bushy Parallelism | O(n) |
| `cpu-affinity-placement` | CPU Affinity Placement | O(n/p) |
| `degree-of-parallelism-selection` | Degree of Parallelism Selection | O(1) |
| `inter-operator-parallelism` | Inter-Operator Parallelism | O(n) |
| `intra-operator-parallelism` | Intra-Operator Parallelism | O(n/p) |
| `morsel-driven-parallelism` | Morsel-Driven Parallelism | O(n/p) |
| `numa-aware-scheduling` | NUMA-Aware Scheduling | O(n/p) |
| `parallel-aggregation` | Parallel Aggregation | O(n/p) |
| `parallel-append` | Parallel Append | O(n/p) |
| `parallel-hash-join` | Parallel Hash Join | O((n+m)/p) |
| `parallel-index-scan` | Parallel Index Scan | O(log n + k/p) |
| `parallel-partition-wise-join` | Parallel Partition-Wise Join | O((n+m)/p) |
| `parallel-scan` | Parallel Scan | O(n/p) |
| `parallel-sort` | Parallel Sort | O((n log n)/p) |
| `parallel-union` | Parallel Union | O(max(n,m)/p) |
| `work-stealing-parallelism` | Work-Stealing Parallelism | O(n/p + log p) |

## Known Duplicate IDs

The following rule IDs appear in multiple directories. These are intentional
cross-references where the same optimization concept appears in different
contexts (e.g., a rule exists in both `experimental/` and `execution-models/`).

| Rule ID | Locations |
|---------|-----------|
| `adaptive-aggregation` | `experimental/adaptive/adaptive-aggregation.rra`, `physical/aggregation-strategies/adaptive-aggregation.rra` |
| `adaptive-indexing` | `execution-models/experimental/adaptive-indexing.rra`, `experimental/adaptive/adaptive-indexing.rra` |
| `adaptive-join-selection` | `execution-models/adaptive/adaptive-join-selection.rra`, `experimental/adaptive/adaptive-join-selection.rra` |
| `approximate-query-processing` | `execution-models/experimental/approximate-query-processing.rra`, `experimental/approximate/approximate-query-processing.rra` |
| `filter-merge` | `database-specific/calcite/filter-merge.rra`, `logical/predicate-pushdown/filter-merge.rra` |
| `gpu-offloading` | `execution-models/experimental/gpu-offloading.rra`, `experimental/hardware-accel/gpu-offloading.rra` |
| `learned-cost-models` | `execution-models/experimental/learned-cost-models.rra`, `experimental/ml-guided/learned-cost-models.rra` |
| `ml-cardinality-estimation` | `execution-models/experimental/ml-cardinality-estimation.rra`, `experimental/ml-guided/ml-cardinality-estimation.rra` |
| `morsel-driven-parallelism` | `execution-models/morsel-driven/morsel-driven-parallelism.rra`, `physical/parallelization/morsel-driven-parallelism.rra` |
| `quantum-inspired-optimization` | `execution-models/experimental/quantum-inspired-optimization.rra`, `experimental/hardware-accel/quantum-inspired-optimization.rra` |
| `query-compilation-jit` | `execution-models/experimental/query-compilation-jit.rra`, `experimental/compilation/query-compilation-jit.rra` |
| `rl-join-ordering` | `execution-models/experimental/rl-join-ordering.rra`, `experimental/ml-guided/rl-join-ordering.rra` |
| `two-phase-aggregation` | `distributed/partial-aggregation/two-phase-aggregation.rra`, `physical/aggregation-strategies/two-phase-aggregation.rra` |
| `window-function-pushdown` | `logical/window-pushdown/window-function-pushdown.rra`, `multi-model/timeseries/window-function-pushdown.rra` |
| `zigzag-join` | `distributed/distributed-joins/zigzag-join.rra`, `physical/join-algorithms/zigzag-join.rra` |
