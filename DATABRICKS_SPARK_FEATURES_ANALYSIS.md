# Databricks/Spark SQL Features Analysis

Comprehensive analysis of Databricks-specific and Spark SQL features not currently supported by the Ra optimizer.

**Date**: 2026-03-28
**Ra Version**: Based on codebase analysis
**Scope**: Features beyond standard SQL present in Databricks Runtime and Apache Spark SQL

---

## Executive Summary

This document catalogs Databricks-specific features and Spark SQL extensions that extend beyond Ra's current optimization capabilities. While Ra provides extensive support for standard relational operations, distributed query optimization, and many database-specific features (PostgreSQL, MySQL, DuckDB, ClickHouse, etc.), it lacks native support for Delta Lake operations, Photon-specific optimizations, and several Spark SQL language extensions.

**Current Ra Capabilities**:
- 1,327+ transformation rules across logical, physical, hardware, distributed, and multi-model categories
- Distributed query optimization with dynamic partition pruning (Spark-compatible)
- Bloom filter pushdown (Spark-compatible)
- Materialized view optimization
- Adaptive execution with runtime plan switching
- Support for 32+ SQL dialects via polyglot backend (including Databricks and Spark SQL)

**Gap Areas**:
1. Delta Lake-specific operations (MERGE, OPTIMIZE, time travel)
2. Liquid clustering and Z-ORDER optimizations
3. Photon engine acceleration
4. Higher-order functions and lambda expressions
5. Unity Catalog integration
6. Streaming tables and change data feed
7. Several Spark SQL syntax extensions

---

## 1. Delta Lake Features

### 1.1 MERGE INTO (Upsert Operations)

**Description**: Atomic operation combining INSERT, UPDATE, and DELETE in a single transaction.

**Syntax**:
```sql
MERGE INTO target_table
USING source_table
ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *
WHEN NOT MATCHED THEN INSERT *
WHEN NOT MATCHED BY SOURCE THEN DELETE
```

**Databricks-Specific Optimizations**:
- Single-row match enforcement (throws error on multiple matches)
- Partition-based filtering with time windows for incremental sync
- Automatic deduplication of new data against existing records
- `updateAll()` and `insertAll()` shorthand operations
- Schema evolution support
- Streaming integration via `foreachBatch`

**Ra Integration Complexity**: **High**
- Requires transaction log implementation
- Needs ACID guarantees
- Complex cost model for multi-operation queries
- Partition-aware optimization strategies

**Optimization Strategies**:
1. Pre-merge source deduplication
2. Partition pruning on date ranges
3. Conditional `WHEN NOT MATCHED BY SOURCE` clauses to avoid full rewrites
4. Cost model: `delta_size * 10.0 + view_size * 0.1` vs full rewrite

**Current Ra Status**: ❌ Not supported
- Ra has no equivalent for multi-operation atomic transactions
- Dialect translator can parse but not optimize

---

### 1.2 Time Travel (AS OF Queries)

**Description**: Query previous table versions using version numbers or timestamps.

**Syntax**:
```sql
-- Timestamp-based
SELECT * FROM sales AS OF TIMESTAMP '2024-01-01 00:00:00'
SELECT * FROM sales VERSION AS OF 42

-- Shorthand notation
SELECT * FROM sales@20240101000000000
SELECT * FROM sales@v42
```

**Features**:
- Timestamp-based queries (ISO 8601 format)
- Version-based queries (transaction log versions)
- RESTORE command to revert tables
- Configurable retention policies

**Configuration**:
- `delta.logRetentionDuration`: Transaction history (default: 30 days)
- `delta.deletedFileRetentionDuration`: Data file retention (default: 7 days)

**Ra Integration Complexity**: **Medium**
- Requires versioned metadata tracking
- Cost model for historical queries
- Integration with storage layer

**Optimization Strategies**:
- Metadata-only queries for schema evolution
- Incremental snapshots vs full copies
- Pruning based on file-level metadata

**Current Ra Status**: ❌ Not supported
- No versioning abstraction in RelExpr
- Could be added as temporal query operators

---

### 1.3 OPTIMIZE and Z-ORDER

**Description**: Compaction and data layout optimization for query performance.

**Syntax**:
```sql
-- Basic compaction
OPTIMIZE table_name

-- With partition filter
OPTIMIZE table_name WHERE date >= '2024-01-01'

-- Z-ORDER by high-cardinality columns
OPTIMIZE table_name ZORDER BY (customer_id, product_id)
```

**Databricks-Specific Enhancements**:
- Liquid clustering integration
- Automatic predictive optimization for Unity Catalog tables
- CPU-intensive Parquet decoding/encoding (recommend compute-optimized instances)
- Idempotent operations

**Cost Model**:
```
saved_scan = pruned_partitions * rows_per_partition * row_bytes
benefit = saved_scan - optimization_cost
```

**Ra Integration Complexity**: **High**
- Physical data layout optimization (beyond logical algebra)
- Storage-level operations
- Multi-dimensional clustering algorithms

**Optimization Strategies**:
1. Off-peak hour scheduling
2. Daily optimization for frequently updated tables
3. Z-ORDER on columns used in WHERE clauses
4. Balance between performance gain and compute cost

**Current Ra Status**: ❌ Not supported
- Ra focuses on logical and physical operator optimization
- Physical file layout is storage layer concern
- Could model as cost factors in scan operators

---

### 1.4 Liquid Clustering

**Description**: Automatic data clustering that adapts to query patterns without partitioning overhead.

**Features**:
- Automatically groups data by clustering keys
- No need to manually define partitions
- Adapts to changing query patterns
- Integrated with OPTIMIZE command

**vs Traditional Partitioning**:
- No partition explosion with high-cardinality columns
- No manual partition management
- Better for multiple query patterns
- Requires fewer OPTIMIZE runs than Z-ORDER

**vs Z-ORDER**:
- More automatic and adaptive
- Better for evolving query patterns
- Lower maintenance overhead

**Ra Integration Complexity**: **Very High**
- Adaptive algorithm based on query workload
- Requires query pattern learning
- Storage-level reorganization

**Current Ra Status**: ❌ Not supported
- No adaptive clustering in Ra
- Could integrate with ML-based cardinality estimation

---

### 1.5 Data Skipping and Predictive I/O

**Description**: Automatic metadata collection for file-level pruning.

**Features**:
- Min/max values per file
- Null counts
- Total record counts
- Predictive optimization for column selection (Unity Catalog)
- Configurable via `dataSkippingStatsColumns` property

**Performance Benefits**:
- Reduces disk I/O by skipping entire files
- Complements partition pruning
- Effective with Z-ORDER co-location

**Ra Integration Complexity**: **Medium**
- Ra has parquet pushdown support
- Needs extension for Delta-specific statistics

**Current Ra Status**: ⚠️ Partial support
- Ra has `parquet_pushdown` module
- Missing Delta-specific statistics layer
- See: `/home/gburd/ws/ra/crates/ra-engine/src/parquet_pushdown.rs`

---

### 1.6 Change Data Feed (CDF)

**Description**: Captures row-level changes (inserts, updates, deletes) for incremental ETL.

**Metadata Columns**:
- `_change_type`: insert, update_preimage, update_postimage, delete
- `_commit_version`: Transaction version
- `_commit_timestamp`: Transaction timestamp

**Use Cases**:
- Incremental ETL pipelines
- Audit logging
- Downstream system synchronization
- Time-series analysis of changes

**Ra Integration Complexity**: **High**
- Requires change tracking infrastructure
- Integration with streaming execution model
- Incremental materialized view maintenance

**Current Ra Status**: ❌ Not supported
- Ra has incremental view maintenance rules
- Missing CDC-specific operators
- Could leverage differential dataflow integration

---

### 1.7 Constraints (NOT NULL, CHECK, Primary/Foreign Keys)

**Description**: Schema constraints enforced at write time.

**Syntax**:
```sql
ALTER TABLE sales ADD CONSTRAINT positive_amount CHECK (amount > 0)
ALTER TABLE sales ALTER COLUMN id SET NOT NULL
```

**Features**:
- NOT NULL constraints
- CHECK constraints with arbitrary expressions
- Primary key declaration (informational, not enforced)
- Foreign key declaration (informational, not enforced)

**Ra Integration Complexity**: **Low to Medium**
- Ra has constraint_optimizer module for PostgreSQL
- Needs extension for Delta Lake constraint types

**Current Ra Status**: ⚠️ Partial support
- Constraint-based optimization exists: `/home/gburd/ws/ra/crates/ra-engine/src/constraint_optimizer.rs`
- Missing Delta Lake-specific constraint validation

---

### 1.8 Generated Columns

**Description**: Columns automatically computed from expressions over other columns.

**Syntax**:
```sql
CREATE TABLE people (
  birthDate TIMESTAMP,
  dateOfBirth DATE GENERATED ALWAYS AS (CAST(birthDate AS DATE))
)
```

**Features**:
- Stored (materialized) values
- Expression validation on insert
- Useful for partitioning derived columns
- Cannot use UDFs, aggregates, or window functions

**Ra Integration Complexity**: **Medium**
- Requires expression evaluation
- Integration with cost models (pre-computed vs on-the-fly)
- Functional dependency tracking

**Current Ra Status**: ❌ Not supported
- Ra has functional dependency analysis
- Missing generated column semantic support

---

### 1.9 Identity Columns

**Description**: Auto-incrementing column values for unique row identification.

**Features**:
- Sequential value generation
- Unique row identifiers
- Integration with generated columns

**Ra Integration Complexity**: **Low**
- Simple semantic extension
- Cardinality and uniqueness tracking

**Current Ra Status**: ❌ Not supported

---

### 1.10 Delta Lake VACUUM

**Description**: Removes old data files no longer referenced by transaction log.

**Syntax**:
```sql
VACUUM table_name [RETAIN num HOURS] [DRY RUN]
```

**Ra Integration Complexity**: **Low** (maintenance operation, not query optimization)

**Current Ra Status**: ❌ Not supported (out of scope for query optimizer)

---

### 1.11 Delta Lake RESTORE

**Description**: Reverts table to previous version.

**Syntax**:
```sql
RESTORE TABLE table_name TO VERSION AS OF 42
RESTORE TABLE table_name TO TIMESTAMP AS OF '2024-01-01'
```

**Ra Integration Complexity**: **Low** (DDL operation)

**Current Ra Status**: ❌ Not supported

---

### 1.12 CLONE Operations

**Description**: Create shallow or deep copies of Delta tables.

**Types**:
- **Shallow Clone**: Metadata copy, references original data files
- **Deep Clone**: Complete data and metadata copy

**Syntax**:
```sql
CREATE TABLE target SHALLOW CLONE source
CREATE TABLE target DEEP CLONE source
```

**Ra Integration Complexity**: **Low** (DDL operation)

**Current Ra Status**: ❌ Not supported

---

### 1.13 UniForm (Universal Format)

**Description**: Enables reading Delta tables as Apache Iceberg format without data rewriting.

**Features**:
- Automatic Iceberg metadata generation
- Single data copy, multiple format interfaces
- Unity Catalog integration required
- Asynchronous metadata updates

**Ra Integration Complexity**: **Very High** (multi-format optimization)

**Current Ra Status**: ❌ Not supported

---

## 2. Photon Engine Optimizations

### 2.1 Photon Vectorized Execution

**Description**: Databricks' proprietary C++ vectorized query engine.

**Accelerated Operations**:
- Scans (Parquet, Delta, JSON, CSV, Avro, ORC)
- Filters and projections
- Joins (hash, broadcast, sort-merge)
- Aggregations
- Window functions
- Sort operations
- String operations
- Decimal arithmetic

**Performance Characteristics**:
- 2-5x faster than standard Spark for typical workloads
- Up to 10x for scan-heavy queries
- Reduced memory footprint

**Ra Integration Complexity**: **Very High**
- Proprietary execution engine
- Hardware-specific optimizations
- Ra would need to model Photon costs separately

**Current Ra Status**: ❌ Not supported
- Ra has hardware-aware optimization for GPU/FPGA/SIMD
- Photon-specific models would require profiling

---

## 3. Higher-Order Functions and Lambda Expressions

### 3.1 Array Higher-Order Functions

**Description**: Functions accepting lambda expressions for array manipulation.

**Supported Functions**:

#### `transform(array, lambda)`
```sql
SELECT transform(array(1, 2, 3), x -> x * 2)
-- Result: [2, 4, 6]
```

#### `filter(array, lambda)`
```sql
SELECT filter(array(1, 2, 3), x -> x % 2 == 1)
-- Result: [1, 3]

SELECT filter(array(0, 2, 3), (x, i) -> x > i)
-- Result: [2, 3]
```

#### `aggregate(array, initial, merge, [finish])`
```sql
SELECT aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x)
-- Result: 6

SELECT aggregate(array(1, 2, 3), 0, (acc, x) -> acc + x, acc -> acc * 10)
-- Result: 60
```

#### `exists(array, lambda)`
```sql
SELECT exists(array(1, 2, 3), x -> x % 2 == 0)
-- Result: true
```

#### `forall(array, lambda)`
```sql
SELECT forall(array(2, 4, 8), x -> x % 2 == 0)
-- Result: true
```

#### `reduce(array, initial, merge)`
```sql
SELECT reduce(array(1, 2, 3), 0, (acc, x) -> acc + x)
-- Result: 6
```

#### `zip_with(array1, array2, lambda)`
```sql
SELECT zip_with(array(1, 2, 3), array(4, 5, 6), (x, y) -> x + y)
-- Result: [5, 7, 9]
```

### 3.2 Map Higher-Order Functions

#### `map_filter(map, lambda)`
```sql
SELECT map_filter(map('a', 1, 'b', 2), (k, v) -> v > 1)
-- Result: {'b': 2}
```

#### `transform_keys(map, lambda)`
```sql
SELECT transform_keys(map('a', 1, 'b', 2), (k, v) -> upper(k))
-- Result: {'A': 1, 'B': 2}
```

#### `transform_values(map, lambda)`
```sql
SELECT transform_values(map('a', 1, 'b', 2), (k, v) -> v * 2)
-- Result: {'a': 2, 'b': 4}
```

#### `map_zip_with(map1, map2, lambda)`
```sql
SELECT map_zip_with(map('a', 1), map('a', 2), (k, v1, v2) -> v1 + v2)
-- Result: {'a': 3}
```

**Lambda Syntax**:
```
x -> expression                    -- Single parameter
(x, y) -> expression               -- Multiple parameters
(x, i) -> expression               -- With index (for filter)
(acc, x) -> expression             -- Accumulator (for aggregate/reduce)
(k, v) -> expression               -- Key-value pairs (for maps)
```

**Ra Integration Complexity**: **Very High**
- Requires lambda expression IR
- Type inference for nested functions
- Cost modeling for functional operations
- Optimization of lambda composition

**Current Ra Status**: ❌ Not supported
- Ra Expr enum has function calls but no lambdas
- Would require significant AST extension

---

## 4. LATERAL VIEW and Explode Functions

### 4.1 LATERAL VIEW (Deprecated in Databricks Runtime 12.2+)

**Description**: Unnests arrays and maps into virtual tables.

**Syntax**:
```sql
SELECT name, tag
FROM people
LATERAL VIEW explode(tags) exploded_table AS tag
```

**With OUTER**:
```sql
SELECT name, tag
FROM people
LATERAL VIEW OUTER explode(tags) exploded_table AS tag
-- Preserves rows with empty arrays (returns NULL)
```

**Ra Integration Complexity**: **Medium**
- Ra has `UnnestExecutor` and `MultiUnnestExecutor`
- Missing LATERAL VIEW-specific semantics

**Current Ra Status**: ⚠️ Partial support
- Unnesting supported: `/home/gburd/ws/ra/crates/ra-engine/src/executors.rs`
- LATERAL VIEW syntax not explicitly handled

---

### 4.2 Generator Functions

**Supported Functions**:
- `explode(array)` - Expands array to rows
- `explode(map)` - Expands map to (key, value) rows
- `posexplode(array)` - Explode with position
- `inline(array<struct>)` - Expands struct array to columns
- `stack(n, col1, col2, ...)` - Pivots N values per row

**Ra Status**: ⚠️ Partial support through unnest operations

---

## 5. PIVOT and UNPIVOT Operations

### 5.1 PIVOT

**Description**: Rotates rows into columns.

**Syntax**:
```sql
SELECT * FROM sales
PIVOT (
  SUM(amount) AS total
  FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4')
)
```

**Result Transformation**:
```
Before:
year | quarter | amount
2024 | Q1      | 100
2024 | Q2      | 150

After:
year | Q1_total | Q2_total | Q3_total | Q4_total
2024 | 100      | 150      | NULL     | NULL
```

**Ra Integration Complexity**: **Medium**
- Logical transformation to GROUP BY + aggregates
- Expression rewriting

**Current Ra Status**: ❌ Not supported
- Could be implemented as rewrite rule
- Equivalent to filtered aggregations

---

### 5.2 UNPIVOT

**Description**: Rotates columns into rows (inverse of PIVOT).

**Syntax**:
```sql
SELECT * FROM quarterly_sales
UNPIVOT (
  amount FOR quarter IN (q1, q2, q3, q4)
)
```

**Ra Integration Complexity**: **Medium**

**Current Ra Status**: ❌ Not supported

---

## 6. Query Hints

### 6.1 Join Hints

**BROADCAST (MAPJOIN, BROADCASTJOIN)**:
```sql
SELECT /*+ BROADCAST(dim_table) */ *
FROM fact_table f
JOIN dim_table d ON f.id = d.id
```

**MERGE (SHUFFLE_MERGE, MERGEJOIN)**:
```sql
SELECT /*+ MERGE(large_table1, large_table2) */ *
FROM large_table1 t1
JOIN large_table2 t2 ON t1.id = t2.id
```

**SHUFFLE_HASH**:
```sql
SELECT /*+ SHUFFLE_HASH(t1) */ *
FROM t1 JOIN t2 ON t1.id = t2.id
```

**SHUFFLE_REPLICATE_NL**:
```sql
SELECT /*+ SHUFFLE_REPLICATE_NL(t1) */ *
FROM t1 JOIN t2 ON t1.val > t2.val
```

**Priority Order**: BROADCAST > MERGE > SHUFFLE_HASH > SHUFFLE_REPLICATE_NL

### 6.2 Partitioning Hints

**COALESCE**:
```sql
SELECT /*+ COALESCE(4) */ * FROM large_table
```

**REPARTITION**:
```sql
SELECT /*+ REPARTITION(10) */ * FROM table
SELECT /*+ REPARTITION(customer_id) */ * FROM table
```

**REPARTITION_BY_RANGE**:
```sql
SELECT /*+ REPARTITION_BY_RANGE(10, date) */ * FROM table
```

**REBALANCE**:
```sql
SELECT /*+ REBALANCE */ * FROM table
```

### 6.3 Skew Hints

**SKEW**:
```sql
SELECT /*+ SKEW('orders', 'customer_id') */ *
FROM orders o
JOIN customers c ON o.customer_id = c.id
```

**Ra Integration Complexity**: **Low to Medium**
- Ra already supports distributed join strategies
- Hints could guide cost model decisions

**Current Ra Status**: ⚠️ Partial support
- Broadcast, shuffle, merge strategies exist in distributed rules
- Missing hint parsing and enforcement
- See: `/home/gburd/ws/ra/rules/distributed/join-distribution/`

---

## 7. Unity Catalog Integration

### 7.1 Three-Level Namespace

**Syntax**: `catalog.schema.table`

**Features**:
- Catalog-level access control
- Cross-catalog queries
- Centralized metadata management

**Ra Integration Complexity**: **Medium**
- Namespace resolution
- Metadata API integration

**Current Ra Status**: ❌ Not supported

---

### 7.2 Row/Column-Level Security

**Description**: Fine-grained access control via SQL policies.

**Features**:
- Row filters based on user identity
- Column masking functions
- Dynamic policy evaluation

**Ra Integration Complexity**: **High**
- Security policy application
- User context in optimization
- Query rewriting for filtered views

**Current Ra Status**: ❌ Not supported
- Ra has column-masking-pushdown rule (logical)
- Missing Unity Catalog-specific integration

---

### 7.3 External Locations and Credentials

**DDL Operations**:
```sql
CREATE EXTERNAL LOCATION cloud_bucket
URL 's3://my-bucket/path'
WITH (CREDENTIAL aws_cred)

CREATE CREDENTIAL aws_cred
WITH (AWS_ACCESS_KEY = '...', AWS_SECRET_KEY = '...')
```

**Ra Integration Complexity**: **Low** (metadata management, not optimization)

**Current Ra Status**: ❌ Not supported

---

### 7.4 Volumes (Managed File Storage)

**Description**: Unity Catalog-managed cloud storage for unstructured data.

**Ra Integration Complexity**: **Low** (out of scope)

**Current Ra Status**: ❌ Not supported

---

## 8. Materialized Views and Streaming Tables

### 8.1 Materialized Views (Databricks Style)

**Syntax**:
```sql
CREATE MATERIALIZED VIEW mv_daily_sales AS
SELECT date, SUM(amount) as total
FROM sales
GROUP BY date
```

**Features**:
- Automatic refresh (via predictive optimization)
- Query rewriting
- Incremental maintenance

**Ra Integration Complexity**: **Medium**
- Ra has extensive MV support
- Missing Databricks-specific refresh mechanisms

**Current Ra Status**: ⚠️ Partial support
- MV matching and rewriting: `/home/gburd/ws/ra/crates/ra-engine/src/mv_matching.rs`
- Incremental maintenance rules exist
- Missing Databricks refresh integration

---

### 8.2 Streaming Tables

**Description**: Continuously updated tables from streaming sources.

**Syntax**:
```sql
CREATE STREAMING TABLE streaming_orders AS
SELECT * FROM cloud_files('/path/to/data')
```

**Features**:
- Continuous ingestion
- Exactly-once processing
- Integrated with Delta Live Tables

**Ra Integration Complexity**: **Very High**
- Streaming query optimization
- Windowing and watermarks
- Stateful operations

**Current Ra Status**: ❌ Not supported
- Ra has streaming execution model support
- Missing Databricks-specific streaming semantics

---

## 9. Adaptive Query Execution (AQE)

### 9.1 Runtime Join Strategy Selection

**Description**: Switches join strategies based on runtime statistics.

**Features**:
- Broadcast join conversion at runtime
- Skew join optimization
- Cost-based shuffle partition coalescing

**Configuration**:
```
spark.sql.adaptive.enabled = true
spark.sql.adaptive.coalescePartitions.enabled = true
spark.sql.adaptive.skewJoin.enabled = true
```

**Ra Integration Complexity**: **Medium**
- Ra has adaptive execution support
- Missing Spark-specific AQE integration

**Current Ra Status**: ⚠️ Partial support
- Progressive re-optimization: `/home/gburd/ws/ra/crates/ra-engine/src/progressive_reopt.rs`
- Adaptive join selection: `/home/gburd/ws/ra/rules/execution-models/adaptive/adaptive-join-selection.rra`
- Missing Spark AQE-specific triggers

---

### 9.2 Dynamic Partition Pruning

**Description**: Runtime partition filtering based on join build side.

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ✅ Supported
- See: `/home/gburd/ws/ra/rules/distributed/partition-pruning/dynamic-partition-pruning.rra`
- Explicitly mentions Spark compatibility

---

### 9.3 Runtime Bloom Filters

**Description**: Builds Bloom filters at runtime for join reduction.

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ✅ Supported
- Bloom filter pushdown: `/home/gburd/ws/ra/rules/logical/sideways-information-passing/bloom-filter-pushdown.rra`
- Runtime filters module: `/home/gburd/ws/ra/crates/ra-engine/src/runtime_filters.rs`

---

## 10. Hive Metastore Compatibility

### 10.1 Hive SerDes

**Description**: Custom serialization/deserialization formats.

**Supported Formats**:
- SequenceFile
- RCFile
- ORC with Hive-specific properties
- Custom SerDes

**Ra Integration Complexity**: **Low** (storage layer concern)

**Current Ra Status**: ❌ Not supported

---

### 10.2 Hive-Style Partitioning

**Description**: Directory-based partitioning (`/year=2024/month=01/`).

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ⚠️ Implicit support
- Ra's partition pruning rules apply
- No Hive-specific awareness

---

## 11. SQL Scripting and Procedural Logic

### 11.1 Control Flow Statements

**Supported Statements**:
- `IF ... THEN ... ELSE`
- `CASE ... WHEN ... THEN ... END`
- `FOR ... DO ... END FOR`
- `WHILE ... DO ... END WHILE`
- `REPEAT ... UNTIL ... END REPEAT`
- `LOOP ... END LOOP`
- `ITERATE` (continue)
- `LEAVE` (break)

**Example**:
```sql
IF quantity > 100 THEN
  SET discount = 0.15;
ELSE
  SET discount = 0.05;
END IF;
```

**Ra Integration Complexity**: **Very High**
- Procedural logic optimization
- Control flow graph analysis
- Loop invariant hoisting

**Current Ra Status**: ❌ Not supported
- Ra focuses on declarative SQL
- Procedural extensions out of scope

---

### 11.2 Variables and Session State

**Syntax**:
```sql
DECLARE quantity INT DEFAULT 0;
SET quantity = 42;
SELECT quantity;
```

**Ra Integration Complexity**: **Medium**

**Current Ra Status**: ❌ Not supported

---

## 12. Named Parameters and Default Values

### 12.1 Named Function Parameters

**Syntax**:
```sql
SELECT add_months(date => '2024-01-01', months => 3)
```

**Ra Integration Complexity**: **Low** (parser extension)

**Current Ra Status**: ❌ Not supported

---

### 12.2 Default Parameter Values

**Syntax**:
```sql
CREATE FUNCTION greet(name STRING DEFAULT 'World') RETURNS STRING
RETURN CONCAT('Hello, ', name);

SELECT greet();  -- 'Hello, World'
SELECT greet('Alice');  -- 'Hello, Alice'
```

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ❌ Not supported

---

## 13. User-Defined Functions (UDFs)

### 13.1 Scalar Python UDFs

**Description**: Row-by-row Python functions (slowest UDF type).

**Ra Integration Complexity**: **Very High** (runtime integration)

**Current Ra Status**: ❌ Not supported

---

### 13.2 Pandas UDFs (Vectorized UDFs)

**Description**: Vectorized Python UDFs using Apache Arrow (up to 100x faster than scalar UDFs).

**Types**:
- Series to Series
- Iterator of Series to Iterator of Series
- Iterator of Multiple Series to Iterator of Series

**Example**:
```python
from pyspark.sql.functions import pandas_udf
import pandas as pd

@pandas_udf("double")
def square(s: pd.Series) -> pd.Series:
    return s * s

df.select(square(col("value")))
```

**Ra Integration Complexity**: **Very High**

**Current Ra Status**: ❌ Not supported
- Could model as black-box operators with cost estimates

---

### 13.3 Aggregate UDFs (UDAFs)

**Description**: User-defined aggregate functions.

**Ra Integration Complexity**: **High**

**Current Ra Status**: ❌ Not supported

---

### 13.4 Table-Valued Functions (UDTFs)

**Description**: Functions returning multiple rows/columns.

**Ra Integration Complexity**: **Medium**

**Current Ra Status**: ⚠️ Partial support
- TableFunctionExecutor exists: `/home/gburd/ws/ra/crates/ra-engine/src/executors.rs`

---

## 14. Additional SQL Extensions

### 14.1 QUALIFY Clause

**Description**: Filters results of window functions directly.

**Syntax**:
```sql
SELECT name, salary, RANK() OVER (ORDER BY salary DESC) as rank
FROM employees
QUALIFY rank <= 10
```

**Equivalent to**:
```sql
SELECT * FROM (
  SELECT name, salary, RANK() OVER (ORDER BY salary DESC) as rank
  FROM employees
) WHERE rank <= 10
```

**Ra Integration Complexity**: **Low** (syntactic sugar)

**Current Ra Status**: ❌ Not supported

---

### 14.2 COPY INTO

**Description**: Bulk data loading from cloud storage.

**Syntax**:
```sql
COPY INTO target_table
FROM 's3://bucket/path/'
FILEFORMAT = PARQUET
```

**Ra Integration Complexity**: **Low** (data loading, not query optimization)

**Current Ra Status**: ❌ Not supported

---

### 14.3 Table-Valued Functions (TVFs)

**Built-in TVFs**:
- `range(start, end, step)` - Generate numeric sequences
- `explode(array)` - Unnest arrays
- `json_tuple(json, keys...)` - Parse JSON
- `inline(array<struct>)` - Expand struct arrays

**Ra Integration Complexity**: **Medium**

**Current Ra Status**: ⚠️ Partial support

---

## 15. Parquet/Delta Format Optimizations

### 15.1 Parquet Predicate Pushdown

**Description**: Push filters to Parquet file reader.

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ✅ Supported
- Parquet pushdown module: `/home/gburd/ws/ra/crates/ra-engine/src/parquet_pushdown.rs`
- Row group filtering
- Min/max statistics usage

---

### 15.2 Column Pruning

**Description**: Read only required columns from columnar formats.

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ✅ Supported
- Column pruning module: `/home/gburd/ws/ra/crates/ra-engine/src/column_pruning.rs`

---

### 15.3 Vectorized Parquet Reader

**Description**: SIMD-optimized Parquet decoding (Photon).

**Ra Integration Complexity**: **Very High** (proprietary)

**Current Ra Status**: ❌ Not supported
- Ra has SIMD rules but not Photon-specific

---

## 16. Distributed Execution Extensions

### 16.1 Skew Handling

**Features**:
- Automatic skew detection
- Salting/splitting hot keys
- Skew-aware join strategies

**Ra Integration Complexity**: **Medium**

**Current Ra Status**: ✅ Supported
- Skew-aware join rules: `/home/gburd/ws/ra/rules/distributed/join-distribution/skew-aware-*.rra`
- Runtime detection and salted partitioning

---

### 16.2 Multi-Stage Aggregations

**Description**: Two-phase and three-phase distributed aggregations.

**Ra Integration Complexity**: **Low**

**Current Ra Status**: ✅ Supported
- Extensive distributed aggregation rules
- See: `/home/gburd/ws/ra/rules/distributed/aggregation/`

---

## 17. Summary Tables

### Features Supported by Ra

| Feature | Ra Status | Notes |
|---------|-----------|-------|
| Dynamic Partition Pruning | ✅ Full | Spark-compatible implementation |
| Bloom Filter Pushdown | ✅ Full | Runtime filter generation |
| Parquet Pushdown | ✅ Full | Row group filtering, statistics |
| Column Pruning | ✅ Full | Projection pushdown |
| Distributed Join Strategies | ✅ Full | Broadcast, shuffle, colocated |
| Skew Handling | ✅ Full | Detection, salting, splitting |
| Multi-Phase Aggregation | ✅ Full | Two and three-phase |
| Adaptive Execution | ⚠️ Partial | Progressive reoptimization exists |
| Materialized Views | ⚠️ Partial | MV matching, missing Databricks refresh |
| Constraints | ⚠️ Partial | PostgreSQL constraints, not Delta |

### Features Not Supported

| Feature Category | Features | Integration Complexity |
|-----------------|----------|----------------------|
| Delta Lake Core | MERGE, OPTIMIZE, Z-ORDER, Liquid Clustering | High to Very High |
| Delta Lake Time Travel | AS OF queries, RESTORE | Medium |
| Delta Lake Advanced | CDF, Generated Columns, UniForm | High to Very High |
| Photon Engine | Vectorized execution, C++ runtime | Very High (proprietary) |
| Higher-Order Functions | transform, filter, aggregate with lambdas | Very High |
| Array/Map Functions | All lambda-based functions | Very High |
| LATERAL VIEW | Unnesting with LATERAL VIEW syntax | Medium |
| PIVOT/UNPIVOT | Row-column transformations | Medium |
| Query Hints | Hint parsing and enforcement | Low to Medium |
| Unity Catalog | Row/column security, 3-level namespace | Medium to High |
| Streaming Tables | Continuous ingestion, stateful operations | Very High |
| SQL Scripting | IF, FOR, WHILE, variables | Very High |
| UDFs | Scalar, Pandas, UDAF UDFs | Very High |
| Named Parameters | Function parameter names, defaults | Low |
| QUALIFY Clause | Window function filtering | Low |

---

## 18. Integration Priority Recommendations

### High Priority (High Value, Low to Medium Complexity)

1. **PIVOT/UNPIVOT**: Common operation, can be rewritten to standard SQL
2. **QUALIFY Clause**: Syntactic sugar, low complexity
3. **Named Parameters**: Parser extension, improves usability
4. **Hint Parsing**: Ra already has strategies, just needs hint enforcement

### Medium Priority (High Value, Medium to High Complexity)

1. **MERGE INTO**: Core Delta Lake feature, high demand
2. **Time Travel**: Valuable for auditing, requires versioning support
3. **Generated Columns**: Useful optimization opportunity
4. **Unity Catalog Row/Column Security**: Growing demand for security

### Low Priority (High Complexity or Limited Impact)

1. **Higher-Order Functions**: Very complex, limited use in OLAP workloads
2. **Photon Optimizations**: Proprietary, requires partnership
3. **Liquid Clustering**: Complex adaptive algorithm
4. **SQL Scripting**: Procedural logic rarely needs optimization
5. **Vectorized UDFs**: Requires runtime integration

### Research Opportunities

1. **Adaptive Clustering**: ML-driven data layout based on query patterns
2. **CDF-Driven Incremental Maintenance**: Leverage change streams for MV refresh
3. **Multi-Format Query Optimization**: Optimize across Delta/Iceberg/Hudi
4. **Hint Learning**: Automatically learn optimal hints from workload

---

## 19. Ra Optimization Opportunities

### Extend Existing Modules

1. **Parquet Pushdown** → Delta Statistics Integration
   - Add Delta min/max/null statistics
   - Integrate with Z-ORDER locality

2. **Progressive Reopt** → Spark AQE Compatibility
   - Add AQE-specific triggers
   - Implement join strategy switching

3. **Constraint Optimizer** → Delta Constraints
   - Support CHECK constraints
   - Leverage NOT NULL for null elimination

4. **MV Matching** → Databricks Predictive Optimization
   - Model automatic refresh cost
   - Integrate with streaming sources

### New Modules Needed

1. **Delta Optimizer**
   - MERGE operation planning
   - Time travel cost modeling
   - Generated column optimization

2. **Lambda Expression IR**
   - Functional expression representation
   - Lambda composition optimization
   - Type inference

3. **Unity Catalog Adapter**
   - Metadata API integration
   - Security policy application
   - Cross-catalog optimization

---

## 20. Conclusion

Ra provides comprehensive optimization for standard SQL and many database-specific features, with particular strength in distributed query optimization, adaptive execution, and physical operator selection. However, Databricks and Spark SQL introduce significant extensions that go beyond traditional RDBMS optimization:

**Key Gaps**:
1. **Delta Lake Operations**: MERGE, time travel, and clustering require storage-level integration
2. **Functional Programming**: Higher-order functions and lambdas need new IR representation
3. **Proprietary Engines**: Photon requires specific cost models or profiling
4. **Procedural Logic**: SQL scripting is orthogonal to declarative optimization

**Strategic Direction**:
- **Short-term**: Add PIVOT/UNPIVOT, QUALIFY, hint parsing (low-hanging fruit)
- **Medium-term**: MERGE optimization, time travel, Unity Catalog integration
- **Long-term**: Research adaptive clustering, lambda optimization, multi-format queries

Ra's architecture (e-graph optimization, rule-based rewrites, cost modeling) provides a solid foundation for extending Databricks/Spark SQL support. The main challenges are:
1. Storage-level integration (Delta Lake)
2. Runtime execution model differences (Photon, streaming)
3. Functional programming constructs (lambdas)

**Recommendation**: Focus on high-value, low-complexity features first (PIVOT, QUALIFY, hints) while researching Delta Lake integration strategies for future releases.
