# Rule System Research References

This document collects all academic papers, system documentation, and research references cited in the rule collection.

## Academic Papers

- Leis et al. "How Good Are Query Optimizers, Really?" VLDB (2015)
- Pirahesh et al. "Extensible/Rule Based Query Rewrite Optimization" SIGMOD (1992)

## Database Systems

### PostgreSQL

Referenced in      473 rules.

- - PostgreSQL: `src/backend/optimizer/path/costsize.c` - seq_page_cost, cpu_tuple_cost
- - PostgreSQL: `src/backend/optimizer/path/costsize.c` (seq_page_cost, random_page_cost)
- - PostgreSQL source: `src/backend/optimizer/path/costsize.c`
- - PostgreSQL: `src/backend/utils/adt/selfuncs.c` - all selectivity functions
- - PostgreSQL: `src/backend/optimizer/path/costsize.c`

### MySQL

Referenced in      303 rules.


### DuckDB

Referenced in      258 rules.

- - DuckDB: `src/optimizer/join_order/join_node.cpp`
- - DuckDB: `src/optimizer/statistics/` propagation framework
- - DuckDB: `src/optimizer/statistics/expression/` (statistics propagation)
- - DuckDB: `src/execution/aggregate_hashtable.cpp` - vectorized hash agg
- - DuckDB: `src/execution/operator/join/physical_iejoin.cpp`

### Oracle

Referenced in      328 rules.


### SQL Server

Referenced in        5 rules.


### MongoDB

Referenced in      143 rules.

- - MongoDB source: `src/mongo/db/query/stage_builder.cpp`
- - MongoDB source: `src/mongo/db/pipeline/document_source_graph_lookup.cpp`
- - MongoDB source: `src/mongo/db/pipeline/document_source_bucket_auto.cpp`
- - MongoDB source: `src/mongo/s/query/cluster_find.cpp`
- - MongoDB source: `src/mongo/db/pipeline/document_source_lookup.cpp`

### CockroachDB

Referenced in      251 rules.


### ClickHouse

Referenced in      250 rules.

- - ClickHouse: `src/Processors/Transforms/AggregatingTransform.cpp`
- - ClickHouse: `src/Functions/FunctionsComparison.h` - SIMD comparisons
- - **Source**: ClickHouse `src/Storages/MergeTree/KeyCondition.cpp`
- - ClickHouse: `src/Storages/MergeTree/KeyCondition.cpp`
- - ClickHouse: `src/Storages/MergeTree/MergeTreeDataSelectExecutor.cpp`

### Apache Calcite

Referenced in       55 rules.


### Apache Spark

Referenced in        9 rules.


### Presto

Referenced in       76 rules.

- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planDistinctAggregation()
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planAggregation()
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/operator/aggregation/partial/PartialAggregation.java
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/iterative/rule/PushTopNThroughExchange.java
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planWindow()

### Trino

Referenced in       88 rules.

- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planDistinctAggregation()
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planAggregation()
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/operator/aggregation/partial/PartialAggregation.java
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/iterative/rule/PushTopNThroughExchange.java
- Presto/Trino: presto-main/src/main/java/com/facebook/presto/sql/planner/optimizations/AddExchanges.java - planWindow()


## Online Resources

- https://arrow.apache.org/docs/format/Columnar.html
- https://arrow.apache.org/docs/format/Columnar.html#physical-memory-layout
- https://clickhouse.com/docs/en/engines/table-engines/mergetree-family
- https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree
- https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#primary-keys-and-indexes-in-queries
- https://clickhouse.com/docs/en/engines/table-engines/mergetree-family/mergetree#projections
- https://clickhouse.com/docs/en/operations/server-configuration-parameters
- https://clickhouse.com/docs/en/optimize/sparse-primary-indexes
- https://clickhouse.com/docs/en/sql-reference/statements/select/prewhere
- https://cwiki.apache.org/confluence/display/FLINK/FLIP-29
- https://dev.mysql.com/doc/refman/8.0/en/glossary.html#glos_covering_index
- https://dev.mysql.com/doc/refman/8.0/en/group-by-optimization.html
- https://docs.microsoft.com/sql/relational-databases/performance/adaptive-query-processing
- https://docs.microsoft.com/sql/t-sql/queries/select-group-by-transact-sql
- https://docs.mongodb.com/manual/changeStreams/
- https://docs.mongodb.com/manual/core/aggregation-pipeline-optimization/
- https://docs.mongodb.com/manual/core/aggregation-pipeline-sharded-collections/
- https://docs.mongodb.com/manual/core/hashed-sharding/
- https://docs.mongodb.com/manual/core/index-intersection/
- https://docs.mongodb.com/manual/core/index-multikey/

## Key Research Topics

- **cardinality estimation**:       76 mentions
- **join ordering**:      109 mentions
- **predicate pushdown**:       82 mentions
- **cost model**:     1233 mentions
- **query optimization**:      223 mentions
- **adaptive**:      722 mentions
- **vectorized**:      646 mentions
- **columnar**:      268 mentions
- **parallel**:     1115 mentions
- **distributed**:      666 mentions

