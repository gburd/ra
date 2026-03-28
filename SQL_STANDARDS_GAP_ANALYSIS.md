# SQL Standards Gap Analysis: Missing Features in Ra Optimizer

**Analysis Date:** 2026-03-28
**SQL Standards Covered:** SQL:2016, SQL:2019, SQL:2023
**Current Ra Version:** Based on main branch analysis

## Executive Summary

This document identifies ALL SQL standard features from SQL:2016, SQL:2019, and SQL:2023 that are not currently supported by the Ra optimizer. The analysis covers parser support, relational algebra representation, optimization rules, and execution capabilities.

**Key Findings:**
- **Total Missing Features:** 45+ major feature groups
- **SQL:2016:** 12 major feature groups missing
- **SQL:2019:** 4 major feature groups missing
- **SQL:2023:** 3 major feature groups missing
- **Other Standard Features:** 26+ additional missing features

**Current Support Summary:**
- ✅ **Implemented:** Row Pattern Recognition (MATCH_RECOGNIZE), Window Functions, CTEs, Standard Aggregates
- ⚠️ **Partial:** Recursive CTEs (basic support), JSON operators (basic JSONB support)
- ❌ **Missing:** JSON functions, PIVOT/UNPIVOT, Temporal tables, Polymorphic functions, GROUPING SETS enhancements, and many more

---

## 1. SQL:2016 Features

### 1.1 JSON Support (SQL/JSON)

**Status:** ❌ Not Supported (except basic JSONB operators)

#### 1.1.1 JSON_TABLE

**Description:** Converts JSON data into a relational table structure.

**Syntax:**
```sql
SELECT *
FROM orders,
     JSON_TABLE(
       order_items,
       '$.items[*]'
       COLUMNS(
         item_id INT PATH '$.id',
         name VARCHAR(100) PATH '$.name',
         quantity INT PATH '$.qty',
         NESTED PATH '$.specs[*]' COLUMNS(
           spec_name VARCHAR(50) PATH '$.name',
           spec_value VARCHAR(50) PATH '$.value'
         )
       )
     ) AS jt;
```

**What it does:**
- Extracts data from JSON documents using JSONPath expressions
- Maps JSON values to typed relational columns
- Supports nested JSON structures with NESTED PATH
- Handles arrays with ordinal column generation
- Enables joins between JSON and relational data

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL 17+ ✅ (proposed, not yet released)
- MySQL 8.0+ ✅
- SQL Server 2016+ ✅ (via OPENJSON)
- MariaDB 10.6+ ✅

**Implementation Complexity:**
- **Parser:** High (10-12 weeks) - New syntax for COLUMNS clause, PATH expressions, NESTED
- **Planner:** High (8-10 weeks) - New TableFunction operator with JSON-specific semantics
- **Optimizer:** Medium (6-8 weeks) - Predicate pushdown into JSON paths, cardinality estimation

**Optimization Opportunities:**
1. **Path predicate pushdown** - Push WHERE clauses into JSONPath filters
2. **Cardinality estimation** - Estimate array sizes from statistics
3. **Index usage** - Use JSON indexes (GIN/JSONB in PostgreSQL) for path extraction
4. **Parallel unnesting** - Parallelize large JSON array processing
5. **Column pruning** - Skip unused JSON fields during extraction

---

#### 1.1.2 JSON_QUERY

**Description:** Extracts a JSON fragment (object or array) from a JSON document.

**Syntax:**
```sql
SELECT JSON_QUERY(data, '$.orders[*].items' WITH ARRAY WRAPPER)
FROM documents;
```

**What it does:**
- Extracts JSON objects or arrays using JSONPath
- Returns JSON (not scalar values)
- WITH ARRAY WRAPPER wraps results in array
- RETURNING clause specifies output type

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via jsonb_path_query) ✅
- MySQL 8.0+ (via JSON_EXTRACT) ✅
- SQL Server 2016+ ✅

**Implementation Complexity:**
- **Parser:** Low (1-2 weeks) - Simple function syntax
- **Planner:** Low (1 week) - Scalar function representation
- **Optimizer:** Low (1 week) - Constant folding, expression simplification

**Optimization Opportunities:**
1. **Constant folding** - Evaluate on literal JSON at compile time
2. **Path simplification** - Optimize JSONPath expressions
3. **Index hints** - Suggest JSON indexes for frequent paths

---

#### 1.1.3 JSON_VALUE

**Description:** Extracts a scalar value from JSON document.

**Syntax:**
```sql
SELECT JSON_VALUE(data, '$.order.total' RETURNING DECIMAL(10,2))
FROM documents;
```

**What it does:**
- Extracts single scalar values from JSON
- Returns SQL scalar types (not JSON)
- Type coercion with RETURNING clause
- Error handling with ON ERROR/ON EMPTY clauses

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via ->> operator or jsonb_path_query_first) ✅
- MySQL 8.0+ (via JSON_UNQUOTE(JSON_EXTRACT())) ✅
- SQL Server 2016+ ✅

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** Low (1 week)
- **Optimizer:** Low (1 week)

**Optimization Opportunities:**
1. **Type-specific extraction** - Use typed extraction when type is known
2. **Predicate pushdown** - Convert JSON predicates to index scans
3. **Expression rewriting** - Simplify nested JSON_VALUE calls

---

#### 1.1.4 JSON_EXISTS

**Description:** Tests whether a JSONPath expression matches anything in a JSON document.

**Syntax:**
```sql
SELECT id
FROM documents
WHERE JSON_EXISTS(data, '$.orders[*]?(@.total > 100)');
```

**What it does:**
- Boolean test for JSONPath existence
- Supports JSONPath filters (@ current object)
- Used in WHERE clauses for filtering

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via jsonb_path_exists) ✅
- MySQL 8.0+ ✅
- SQL Server (partial via ISJSON) ⚠️

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** Medium (2 weeks) - Predicate representation
- **Optimizer:** Medium (3-4 weeks) - Index scan conversion, selectivity estimation

**Optimization Opportunities:**
1. **Index scan conversion** - Convert to JSONB index scans in PostgreSQL
2. **Selectivity estimation** - Estimate filter selectivity from statistics
3. **Expression correlation** - Track correlation between JSON predicates
4. **Early filtering** - Push JSON_EXISTS before expensive operations

---

#### 1.1.5 JSON_ARRAY

**Description:** Constructs a JSON array from SQL values.

**Syntax:**
```sql
SELECT JSON_ARRAY(id, name, price ORDER BY price DESC NULL ON NULL)
FROM products;
```

**What it does:**
- Creates JSON arrays from SQL expressions
- Supports NULL handling (NULL ON NULL, ABSENT ON NULL)
- ORDER BY for deterministic ordering
- Type conversion from SQL to JSON

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via json_build_array/jsonb_build_array) ✅
- MySQL 8.0+ ✅
- SQL Server (via FOR JSON) ✅

**Implementation Complexity:**
- **Parser:** Medium (2 weeks)
- **Planner:** Low (1 week)
- **Optimizer:** Low (1 week)

**Optimization Opportunities:**
1. **Constant folding** - Build arrays at compile time when inputs are constants
2. **Aggregation fusion** - Combine with array aggregates

---

#### 1.1.6 JSON_OBJECT

**Description:** Constructs a JSON object from key-value pairs.

**Syntax:**
```sql
SELECT JSON_OBJECT(
  'id': id,
  'name': name,
  'price': price,
  NULL ON NULL
)
FROM products;
```

**What it does:**
- Creates JSON objects from SQL key-value pairs
- Keys must be strings
- Supports NULL handling options
- Type conversion for values

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via json_build_object/jsonb_build_object) ✅
- MySQL 8.0+ ✅
- SQL Server (via FOR JSON) ✅

**Implementation Complexity:**
- **Parser:** Medium (2 weeks)
- **Planner:** Low (1 week)
- **Optimizer:** Low (1 week)

**Optimization Opportunities:**
1. **Constant folding** - Build objects at compile time
2. **Column pruning** - Skip unused object fields

---

#### 1.1.7 JSON_ARRAYAGG

**Description:** Aggregate function that creates a JSON array from aggregated values.

**Syntax:**
```sql
SELECT category, JSON_ARRAYAGG(name ORDER BY price DESC)
FROM products
GROUP BY category;
```

**What it does:**
- Aggregate multiple rows into JSON array
- Like ARRAY_AGG but produces JSON
- Supports ORDER BY within aggregate
- NULL handling options

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via json_agg/jsonb_agg) ✅
- MySQL 8.0+ ✅
- SQL Server 2017+ ✅

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** Low (1 week) - Add to AggregateFunction enum
- **Optimizer:** Medium (2-3 weeks) - ORDER BY handling within aggregate

**Optimization Opportunities:**
1. **Parallel aggregation** - Parallelize JSON array construction
2. **Merge aggregates** - Combine multiple JSON_ARRAYAGG with same GROUP BY
3. **Order optimization** - Leverage existing sort order

---

#### 1.1.8 JSON_OBJECTAGG

**Description:** Aggregate function that creates a JSON object from key-value pairs.

**Syntax:**
```sql
SELECT category, JSON_OBJECTAGG(name: price)
FROM products
GROUP BY category;
```

**What it does:**
- Aggregate key-value pairs into JSON object
- Keys must be unique within group
- Supports NULL handling

**Database Support:**
- Oracle 12c+ ✅
- PostgreSQL (via json_object_agg/jsonb_object_agg) ✅
- MySQL 8.0+ ✅
- SQL Server 2017+ ✅

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** Low (1 week)
- **Optimizer:** Medium (2 weeks)

**Optimization Opportunities:**
1. **Duplicate key detection** - Warn about non-unique keys at planning time
2. **Parallel aggregation** - Parallelize object construction

---

### 1.2 Row Pattern Recognition (MATCH_RECOGNIZE)

**Status:** ✅ **IMPLEMENTED** (RFC 0001)

The Ra optimizer has full support for SQL:2016 Row Pattern Recognition including:
- Pattern expressions with quantifiers
- DEFINE and MEASURES clauses
- Window-like partitioning and ordering
- Multiple skip modes
- Optimization rules for pattern simplification and translation to window functions

---

### 1.3 Polymorphic Table Functions (PTF)

**Status:** ❌ Not Supported

**Description:** Table functions that can return different column sets based on input arguments.

**Syntax:**
```sql
CREATE FUNCTION top_n(input_table TABLE, n INT)
  RETURNS TABLE
AS
BEGIN
  RETURN SELECT * FROM input_table LIMIT n;
END;

-- Usage
SELECT * FROM top_n(TABLE(SELECT * FROM orders), 10);
```

**What it does:**
- Functions that operate on entire tables (not just scalar values)
- Return type determined at query planning time
- Input tables passed as arguments
- Used for data transformations, windowing, sampling

**Database Support:**
- Oracle 18c+ ✅
- PostgreSQL (via RETURNS TABLE) ⚠️ (limited support)
- SQL Server ❌
- IBM DB2 ✅

**Implementation Complexity:**
- **Parser:** High (8-10 weeks) - TABLE constructor syntax, polymorphic signatures
- **Planner:** Very High (12-15 weeks) - Type inference, schema propagation
- **Optimizer:** Medium (4-6 weeks) - Inline small PTFs, predicate pushdown through PTFs

**Optimization Opportunities:**
1. **Inlining** - Inline simple PTFs into main query
2. **Predicate pushdown** - Push predicates through PTF boundary
3. **Column pruning** - Prune unused columns from PTF input
4. **Parallel execution** - Parallelize PTF execution
5. **Caching** - Cache PTF results for reuse within query

---

### 1.4 LISTAGG Enhancements

**Status:** ⚠️ Partial (STRING_AGG supported, but not all LISTAGG features)

**Description:** Enhanced string aggregation with overflow handling and ON OVERFLOW clause.

**Syntax:**
```sql
SELECT department,
       LISTAGG(name, ', '
               ON OVERFLOW TRUNCATE '...' WITH COUNT)
         WITHIN GROUP (ORDER BY salary DESC)
FROM employees
GROUP BY department;
```

**What it does:**
- String aggregation (similar to STRING_AGG)
- ON OVERFLOW clause handles result exceeding max length
- TRUNCATE with optional suffix
- WITH COUNT adds count of truncated values
- ERROR option to raise error on overflow

**Database Support:**
- Oracle 11g+ ✅ (enhancements in 12c+)
- PostgreSQL (via STRING_AGG) ⚠️ (no overflow handling)
- SQL Server (via STRING_AGG) ⚠️ (no overflow handling)
- MySQL (via GROUP_CONCAT) ⚠️ (no overflow handling)

**Implementation Complexity:**
- **Parser:** Medium (2-3 weeks) - ON OVERFLOW clause parsing
- **Planner:** Low (1 week) - Extend AggregateExpr
- **Optimizer:** Low (1-2 weeks)

**Optimization Opportunities:**
1. **Length estimation** - Estimate result length to choose truncation strategy
2. **Parallel aggregation** - Merge partial string aggregates

---

### 1.5 WINDOW Clause

**Status:** ❌ Not Supported (Named Windows)

**Description:** Named window definitions that can be referenced by multiple window functions.

**Syntax:**
```sql
SELECT
  name,
  ROW_NUMBER() OVER w,
  RANK() OVER w,
  AVG(salary) OVER w
FROM employees
WINDOW w AS (PARTITION BY department ORDER BY salary DESC);
```

**What it does:**
- Define window specification once, reference multiple times
- Reduces duplication in queries with many window functions
- Improves readability and maintainability

**Database Support:**
- PostgreSQL 8.4+ ✅
- MySQL 8.0+ ✅
- SQL Server 2012+ ✅
- Oracle 11g+ ✅

**Implementation Complexity:**
- **Parser:** Medium (2-3 weeks) - WINDOW clause parsing, reference resolution
- **Planner:** Low (1 week) - Expand named windows to full specifications
- **Optimizer:** Medium (2-3 weeks) - Window sharing with named windows

**Optimization Opportunities:**
1. **Window sharing** - Automatically merge identical window specifications
2. **Sort reuse** - Reuse sort order across multiple windows with same ORDER BY

---

### 1.6 RESPECT NULLS / IGNORE NULLS in Window Functions

**Status:** ❌ Not Supported

**Description:** Control NULL handling in window functions LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE.

**Syntax:**
```sql
SELECT
  date,
  value,
  LAST_VALUE(value) IGNORE NULLS OVER (ORDER BY date)
FROM time_series;
```

**What it does:**
- IGNORE NULLS - Skip NULL values when finding nth value
- RESPECT NULLS - Include NULL values (default)
- Critical for sparse time series data
- Affects LAG, LEAD, FIRST_VALUE, LAST_VALUE, NTH_VALUE

**Database Support:**
- Oracle 11g+ ✅
- PostgreSQL ❌ (not supported)
- SQL Server ✅ (2012+)
- Snowflake ✅

**Implementation Complexity:**
- **Parser:** Low (1 week) - Add IGNORE NULLS / RESPECT NULLS keywords
- **Planner:** Low (1 week) - Add flag to WindowExpr
- **Optimizer:** Low (1 week) - Pass through to execution

**Optimization Opportunities:**
1. **Sparse data detection** - Detect sparse columns and suggest IGNORE NULLS
2. **NULL filtering** - Optimize scans when IGNORE NULLS is specified

---

## 2. SQL:2019 Features

### 2.1 Multi-Dimensional Arrays

**Status:** ❌ Not Supported

**Description:** Arrays with multiple dimensions, like matrices or tensors.

**Syntax:**
```sql
-- Define a 2D array type
CREATE TYPE matrix AS INT ARRAY[10][10];

-- Use multi-dimensional arrays
SELECT sales_data[year][quarter][region]
FROM analytics;
```

**What it does:**
- Arrays with 2+ dimensions
- Element access with multiple indices
- Array slicing: `arr[1:3][2:5]`
- Used for scientific computing, analytics, ML features

**Database Support:**
- PostgreSQL ❌ (only 1D arrays)
- Oracle ❌
- SQL Server ❌
- Specialized databases (MonetDB, SciDB) ✅

**Implementation Complexity:**
- **Parser:** High (6-8 weeks) - Multi-dimensional syntax, slicing
- **Planner:** Very High (10-12 weeks) - Multi-dimensional type system
- **Optimizer:** High (8-10 weeks) - Slice pushdown, dimension reduction

**Optimization Opportunities:**
1. **Slice pushdown** - Push array slicing close to storage
2. **Dimension reduction** - Detect unused dimensions and prune
3. **Vectorization** - SIMD operations on array elements
4. **Parallel processing** - Parallelize operations across array dimensions

---

### 2.2 LISTAGG Improvements (DISTINCT and multi-column)

**Status:** ❌ Not Supported

**Description:** LISTAGG with DISTINCT keyword and support for multiple columns.

**Syntax:**
```sql
-- DISTINCT support
SELECT department,
       LISTAGG(DISTINCT role, ', ') WITHIN GROUP (ORDER BY role)
FROM employees
GROUP BY department;

-- Multi-column (Oracle 19c+)
SELECT department,
       LISTAGG(first_name || ' ' || last_name, ', ')
         WITHIN GROUP (ORDER BY last_name)
FROM employees
GROUP BY department;
```

**Database Support:**
- Oracle 19c+ ✅
- PostgreSQL (via STRING_AGG with DISTINCT) ✅
- SQL Server ❌

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** Medium (2 weeks) - DISTINCT handling in aggregates
- **Optimizer:** Low (1 week)

**Optimization Opportunities:**
1. **Hash-based distinct** - Use hash table for deduplication
2. **Sort-based distinct** - Use sort order for deduplication if already sorted

---

### 2.3 PERIOD Predicates (Temporal)

**Status:** ❌ Not Supported

**Description:** Predicates for comparing time periods (OVERLAPS, CONTAINS, PRECEDES, etc.).

**Syntax:**
```sql
SELECT * FROM reservations r1, reservations r2
WHERE r1.reservation_period OVERLAPS r2.reservation_period
  AND r1.room_id = r2.room_id;
```

**What it does:**
- Period data type (start, end timestamps)
- OVERLAPS - Periods have any overlap
- CONTAINS - One period fully contains another
- PRECEDES - One period ends before another starts
- SUCCEEDS - One period starts after another ends

**Database Support:**
- SQL Server (via temporal tables) ⚠️
- Oracle (via period data type) ✅
- PostgreSQL (via range types) ⚠️
- IBM DB2 ✅

**Implementation Complexity:**
- **Parser:** Medium (3-4 weeks) - Period constructors and predicates
- **Planner:** Medium (3-4 weeks) - Period type representation
- **Optimizer:** Medium (4-5 weeks) - Interval tree indexes, overlap join optimization

**Optimization Opportunities:**
1. **Interval tree indexes** - Use specialized indexes for period overlap queries
2. **Period join optimization** - Specialized join algorithms for period overlaps
3. **Predicate simplification** - Simplify complex period predicates
4. **Statistics** - Gather statistics on period distributions

---

### 2.4 Extended GROUP BY (GROUPING SETS, CUBE, ROLLUP enhancements)

**Status:** ❌ Not Supported (basic GROUP BY only)

**Description:** GROUPING SETS, CUBE, ROLLUP for multi-level aggregation.

**Syntax:**
```sql
-- GROUPING SETS
SELECT region, product, SUM(sales)
FROM sales
GROUP BY GROUPING SETS ((region, product), (region), (product), ());

-- CUBE (all combinations)
SELECT region, product, year, SUM(sales)
FROM sales
GROUP BY CUBE (region, product, year);

-- ROLLUP (hierarchical)
SELECT region, product, SUM(sales)
FROM sales
GROUP BY ROLLUP (region, product);
```

**What it does:**
- GROUPING SETS - Explicitly list grouping combinations
- CUBE - All possible grouping combinations (2^n groups)
- ROLLUP - Hierarchical grouping (n+1 groups)
- GROUPING() function - Identify which columns are grouped
- GROUPING_ID() - Bitmask of grouping columns

**Database Support:**
- Oracle 9i+ ✅
- SQL Server 2008+ ✅
- PostgreSQL 9.5+ ✅
- MySQL 8.0+ ✅
- Snowflake ✅

**Implementation Complexity:**
- **Parser:** High (6-8 weeks) - Complex GROUP BY syntax
- **Planner:** Very High (10-12 weeks) - Multiple aggregation levels in single query
- **Optimizer:** Very High (12-15 weeks) - Shared aggregation computation, sort optimization

**Optimization Opportunities:**
1. **Shared computation** - Compute finer groups once, aggregate up for coarser groups
2. **Sort optimization** - Single sort can produce multiple grouping levels
3. **Parallel grouping** - Parallelize across grouping sets
4. **Materialized views** - Pre-compute common CUBE/ROLLUP combinations
5. **Cardinality estimation** - Accurate estimates for multi-level grouping

**Example Query Rewrite:**
```sql
-- Original (3 separate queries)
SELECT region, SUM(sales) FROM sales GROUP BY region
UNION ALL
SELECT product, SUM(sales) FROM sales GROUP BY product
UNION ALL
SELECT NULL, SUM(sales) FROM sales;

-- Optimized (1 query with shared scan)
SELECT region, product, SUM(sales)
FROM sales
GROUP BY GROUPING SETS ((region), (product), ());
```

---

## 3. SQL:2023 Features

### 3.1 SQL/PGQ (Property Graph Queries)

**Status:** ❌ Not Supported (RFC 0093 mentioned but not found in current codebase)

**Description:** Graph pattern matching using GRAPH_TABLE and graph query language.

**Syntax:**
```sql
-- Find friends-of-friends
SELECT person1.name, person2.name
FROM GRAPH_TABLE (
  social_graph
  MATCH (p1:Person)-[:KNOWS]->(p2:Person)-[:KNOWS]->(p3:Person)
  WHERE p1.id = 123 AND p1 != p3
  COLUMNS (p1.name AS person1, p3.name AS person2)
);

-- Find shortest path
SELECT src.name, dst.name, path_length
FROM GRAPH_TABLE (
  road_network
  MATCH SHORTEST (src:City)-[:ROAD]->*(dst:City)
  WHERE src.name = 'New York' AND dst.name = 'Los Angeles'
  COLUMNS (src.name AS start_city, dst.name AS end_city, path_length)
);
```

**What it does:**
- Query graph data using pattern matching syntax
- Match nodes and edges with filters
- Variable-length paths (Kleene star)
- Shortest path queries
- Graph algorithms (centrality, community detection)
- Integrates with relational data

**Database Support:**
- Oracle 23c ✅
- SQL Server (via MATCH in graph tables) ✅
- PostgreSQL (via Apache AGE extension) ⚠️
- Neo4j (Cypher, similar syntax) ✅

**Implementation Complexity:**
- **Parser:** Very High (15-20 weeks) - Complex graph pattern syntax
- **Planner:** Very High (20-25 weeks) - Graph algebra operators, path finding
- **Optimizer:** Very High (20-25 weeks) - Graph-specific optimizations, join ordering for graphs

**Optimization Opportunities:**
1. **Index selection** - Use graph-specific indexes (adjacency lists, edge indexes)
2. **Path finding algorithms** - Choose between BFS, DFS, Dijkstra, A*
3. **Bidirectional search** - Search from both ends for shortest path
4. **Graph pruning** - Prune graph based on filters before pattern matching
5. **Subgraph extraction** - Extract relevant subgraph before processing
6. **Materialized paths** - Pre-compute and cache common paths
7. **Parallel graph traversal** - Parallelize graph exploration

---

### 3.2 JSON Enhancements

**Status:** ❌ Not Supported

#### 3.2.1 JSON_SERIALIZE

**Description:** Convert JSON value to string with formatting options.

**Syntax:**
```sql
SELECT JSON_SERIALIZE(data RETURNING VARCHAR(1000) FORMAT JSON)
FROM documents;
```

**Database Support:**
- Oracle 21c+ ✅
- SQL Server 2022+ ✅

**Implementation Complexity:** Low (1-2 weeks per function)

---

#### 3.2.2 JSON_SCALE

**Description:** Get precision/scale information for JSON numbers.

---

#### 3.2.3 JSON Type (distinct from VARCHAR)

**Description:** Actual JSON data type (not text with validation).

**Database Support:**
- PostgreSQL (JSONB) ✅
- MySQL 8.0+ ✅
- Oracle 21c+ ✅

---

### 3.3 UNIQUE Predicate (Null Handling)

**Status:** ❌ Not Supported

**Description:** Enhanced UNIQUE constraint with proper NULL handling per SQL:2023.

**Syntax:**
```sql
-- SQL:2023 NULL handling
ALTER TABLE users ADD CONSTRAINT uk_email UNIQUE NULLS NOT DISTINCT (email);
```

**What it does:**
- NULLS DISTINCT - Multiple NULLs allowed (traditional behavior)
- NULLS NOT DISTINCT - Only one NULL allowed
- Fixes ambiguity in UNIQUE constraint NULL handling

**Database Support:**
- PostgreSQL 15+ ✅
- Oracle ❌
- SQL Server ❌

**Implementation Complexity:**
- **Parser:** Low (1 week)
- **Planner:** N/A (DDL, not query optimization)
- **Optimizer:** Low (1 week) - Constraint awareness for redundant DISTINCT elimination

---

## 4. Other Missing Standard SQL Features

### 4.1 PIVOT and UNPIVOT

**Status:** ❌ Not Supported

**Description:** Transform rows to columns (PIVOT) and columns to rows (UNPIVOT).

**Syntax:**
```sql
-- PIVOT: Rows to columns
SELECT *
FROM (
  SELECT region, product, sales
  FROM sales_data
)
PIVOT (
  SUM(sales)
  FOR product IN ('Product A' AS a, 'Product B' AS b, 'Product C' AS c)
);

-- UNPIVOT: Columns to rows
SELECT *
FROM quarterly_sales
UNPIVOT (
  sales FOR quarter IN (q1, q2, q3, q4)
);
```

**What it does:**
- PIVOT - Rotate rows into columns (wide format)
- UNPIVOT - Rotate columns into rows (long format)
- Common in reporting and analytics
- Syntactic sugar for CASE + GROUP BY (PIVOT) and UNION ALL (UNPIVOT)

**Database Support:**
- Oracle 11g+ ✅
- SQL Server 2005+ ✅
- PostgreSQL (via crosstab extension) ⚠️
- MySQL ❌ (manual CASE statements)

**Implementation Complexity:**
- **Parser:** High (6-8 weeks) - Complex syntax with dynamic column lists
- **Planner:** High (8-10 weeks) - Rewrite to CASE + GROUP BY / UNION ALL
- **Optimizer:** Medium (4-6 weeks) - Optimize rewritten queries

**Optimization Opportunities:**
1. **Early rewriting** - Rewrite PIVOT/UNPIVOT early in optimization pipeline
2. **Column pruning** - Prune unused pivoted columns
3. **Predicate pushdown** - Push filters before PIVOT/UNPIVOT
4. **Aggregate optimization** - Apply aggregate optimizations to PIVOTed queries
5. **Cardinality estimation** - Estimate result cardinality for PIVOTs

**Example Rewrite:**
```sql
-- PIVOT query
SELECT * FROM sales
PIVOT (SUM(amount) FOR month IN ('Jan', 'Feb', 'Mar'));

-- Rewritten to
SELECT
  customer,
  SUM(CASE WHEN month = 'Jan' THEN amount END) AS Jan,
  SUM(CASE WHEN month = 'Feb' THEN amount END) AS Feb,
  SUM(CASE WHEN month = 'Mar' THEN amount END) AS Mar
FROM sales
GROUP BY customer;
```

---

### 4.2 Temporal Tables (System-Versioned Tables)

**Status:** ❌ Not Supported

**Description:** Tables with automatic history tracking and time-travel queries.

**Syntax:**
```sql
-- Query historical data
SELECT * FROM employees
  FOR SYSTEM_TIME AS OF '2023-01-01';

-- Query changes over period
SELECT * FROM employees
  FOR SYSTEM_TIME FROM '2023-01-01' TO '2023-12-31';

-- Query all versions
SELECT * FROM employees
  FOR SYSTEM_TIME ALL;
```

**What it does:**
- Automatic versioning of rows on UPDATE/DELETE
- Time-travel queries (AS OF, FROM..TO, BETWEEN..AND)
- History table automatically maintained
- Temporal joins (join at specific point in time)

**Database Support:**
- SQL Server 2016+ ✅
- Oracle (via Flashback Query) ✅
- PostgreSQL (via temporal_tables extension) ⚠️
- MariaDB 10.3+ ✅
- IBM DB2 10.1+ ✅

**Implementation Complexity:**
- **Parser:** High (8-10 weeks) - FOR SYSTEM_TIME clauses
- **Planner:** Very High (12-15 weeks) - History table joins, temporal predicates
- **Optimizer:** High (10-12 weeks) - Temporal index usage, history pruning

**Optimization Opportunities:**
1. **History pruning** - Prune history table based on time predicates
2. **Temporal indexes** - Use specialized indexes on validity periods
3. **Snapshot isolation** - Leverage snapshot isolation for consistent time-travel
4. **Partition pruning** - Partition history tables by time range
5. **Predicate pushdown** - Push temporal predicates to history table scans
6. **Temporal join optimization** - Specialized joins for temporal queries

---

### 4.3 MERGE Statement Enhancements

**Status:** ⚠️ Partial (basic MERGE may be supported via dialect translation, but not parsed)

**Description:** Enhanced MERGE with DELETE clause and multiple WHEN clauses.

**Syntax:**
```sql
MERGE INTO inventory target
USING daily_updates source
ON target.product_id = source.product_id
WHEN MATCHED AND source.quantity = 0 THEN
  DELETE
WHEN MATCHED THEN
  UPDATE SET target.quantity = source.quantity
WHEN NOT MATCHED THEN
  INSERT (product_id, quantity) VALUES (source.product_id, source.quantity);
```

**What it does:**
- Conditional INSERT, UPDATE, DELETE in single statement
- Multiple WHEN MATCHED clauses with different conditions
- WHEN MATCHED...DELETE for conditional deletion
- Atomic operation for upsert logic

**Database Support:**
- Oracle 9i+ ✅
- SQL Server 2008+ ✅
- PostgreSQL 15+ ✅ (as INSERT...ON CONFLICT + DELETE)
- MySQL ❌ (use INSERT...ON DUPLICATE KEY)

**Implementation Complexity:**
- **Parser:** High (6-8 weeks)
- **Planner:** High (8-10 weeks) - Represent as combination of Join + conditional DML
- **Optimizer:** Medium (4-6 weeks) - Join optimization, predicate analysis

---

### 4.4 LATERAL Subqueries

**Status:** ❌ Not Supported (no evidence in codebase)

**Description:** Correlated subqueries in FROM clause that can reference earlier FROM items.

**Syntax:**
```sql
SELECT d.name, e.emp_name, e.salary
FROM departments d,
     LATERAL (
       SELECT name AS emp_name, salary
       FROM employees e
       WHERE e.dept_id = d.id
       ORDER BY salary DESC
       LIMIT 3
     ) e;
```

**What it does:**
- Subquery in FROM can reference previous FROM items
- Like correlated subquery but returns multiple rows
- Used for top-N per group queries
- Can be optimized better than WHERE EXISTS subqueries

**Database Support:**
- PostgreSQL 9.3+ ✅
- Oracle 12c+ (via CROSS APPLY / OUTER APPLY) ✅
- SQL Server (via CROSS APPLY / OUTER APPLY) ✅
- MySQL 8.0+ ✅

**Implementation Complexity:**
- **Parser:** Medium (3-4 weeks) - LATERAL keyword and syntax
- **Planner:** High (8-10 weeks) - Correlated lateral joins, parameter passing
- **Optimizer:** High (10-12 weeks) - Decorrelation, loop join vs hash join selection

**Optimization Opportunities:**
1. **Decorrelation** - Convert LATERAL to regular join when possible
2. **Index usage** - Use indexes on correlated columns
3. **Nested loop vs hash join** - Choose based on cardinality
4. **Parallel execution** - Parallelize LATERAL subquery evaluation
5. **Memoization** - Cache LATERAL results for repeated parameters

---

### 4.5 VALUES with Multiple Rows (Table Value Constructor)

**Status:** ✅ Supported (VALUES operator exists in algebra)

---

### 4.6 FETCH FIRST / OFFSET

**Status:** ✅ Supported (LIMIT/OFFSET equivalent)

---

### 4.7 WITH TIES (in TOP/LIMIT queries)

**Status:** ❌ Not Supported

**Description:** Include tied rows when limiting results with ORDER BY.

**Syntax:**
```sql
SELECT TOP 10 WITH TIES name, score
FROM players
ORDER BY score DESC;

-- Or standard syntax
SELECT name, score
FROM players
ORDER BY score DESC
FETCH FIRST 10 ROWS WITH TIES;
```

**What it does:**
- Returns all rows that tie with the last row
- Example: TOP 10 with 3 players tied at 10th place returns 12 rows
- Only makes sense with ORDER BY

**Database Support:**
- SQL Server ✅
- Oracle ✅
- PostgreSQL ❌
- MySQL ❌

**Implementation Complexity:**
- **Parser:** Low (1-2 weeks)
- **Planner:** Medium (3-4 weeks) - Extend Limit operator
- **Optimizer:** Low (1 week)

**Optimization Opportunities:**
1. **Early termination** - Stop scanning after retrieving enough + ties
2. **Index usage** - Use index for ORDER BY + WITH TIES

---

### 4.8 TABLESAMPLE

**Status:** ❌ Not Supported

**Description:** Sample random subset of table rows.

**Syntax:**
```sql
-- System sampling (page-level)
SELECT * FROM large_table TABLESAMPLE SYSTEM (10);

-- Bernoulli sampling (row-level)
SELECT * FROM large_table TABLESAMPLE BERNOULLI (1) REPEATABLE (123);
```

**What it does:**
- SYSTEM - Block/page-level sampling (faster, less random)
- BERNOULLI - Row-level sampling (slower, more random)
- REPEATABLE - Deterministic sampling with seed
- Used for approximate queries, testing, profiling

**Database Support:**
- PostgreSQL 9.5+ ✅
- SQL Server ✅
- Oracle ✅
- MySQL ❌

**Implementation Complexity:**
- **Parser:** Medium (2-3 weeks)
- **Planner:** Medium (3-4 weeks) - Sampling strategies
- **Optimizer:** Low (2 weeks) - Adjust cardinality estimates

**Optimization Opportunities:**
1. **Cardinality estimation** - Adjust estimates for sampled queries
2. **Index skip** - Skip index scan for SYSTEM sampling
3. **Parallel sampling** - Parallelize sampling across workers

---

### 4.9 FILTER Clause (Aggregate Filters)

**Status:** ❌ Not Supported

**Description:** Filter clause on individual aggregates (alternative to CASE WHEN).

**Syntax:**
```sql
SELECT
  COUNT(*) FILTER (WHERE status = 'active') AS active_count,
  SUM(amount) FILTER (WHERE status = 'completed') AS completed_sum
FROM orders;
```

**What it does:**
- Apply WHERE condition to specific aggregate
- Cleaner than CASE WHEN for conditional aggregates
- Standard SQL:2003 feature
- More efficient than CASE in some databases

**Database Support:**
- PostgreSQL 9.4+ ✅
- SQLite ✅
- Oracle ❌ (use CASE WHEN)
- SQL Server ❌ (use CASE WHEN)
- MySQL ❌ (use CASE WHEN)

**Implementation Complexity:**
- **Parser:** Low (1-2 weeks)
- **Planner:** Low (1 week) - Add filter to AggregateExpr
- **Optimizer:** Low (1 week) - Predicate simplification on filters

**Optimization Opportunities:**
1. **Filter pushdown** - Push filters to scan when possible
2. **Shared computation** - Share scan for multiple filtered aggregates
3. **Rewrite to CASE** - Rewrite for databases without FILTER support

---

### 4.10 Hypothetical-Set Aggregate Functions

**Status:** ❌ Not Supported

**Description:** RANK, DENSE_RANK, PERCENT_RANK, CUME_DIST as ordered-set aggregates.

**Syntax:**
```sql
-- What would be the rank of value 500?
SELECT
  RANK(500) WITHIN GROUP (ORDER BY salary) AS hypothetical_rank
FROM employees;

-- Percentile calculations
SELECT
  PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY salary) AS median_salary
FROM employees;
```

**What it does:**
- RANK/DENSE_RANK/etc. as aggregate functions (not window functions)
- Answer "what-if" questions about ranking
- WITHIN GROUP specifies ordering
- Includes PERCENTILE_CONT, PERCENTILE_DISC

**Database Support:**
- PostgreSQL 9.4+ ✅
- Oracle 9i+ ✅
- SQL Server ❌
- MySQL ❌

**Implementation Complexity:**
- **Parser:** Medium (2-3 weeks) - WITHIN GROUP syntax
- **Planner:** Medium (3-4 weeks) - Ordered-set aggregates
- **Optimizer:** Low (2 weeks)

---

### 4.11 Inverse Distribution Functions

**Status:** ❌ Not Supported

**Description:** PERCENTILE_CONT, PERCENTILE_DISC, MODE for distribution analysis.

**Syntax:**
```sql
SELECT
  PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY salary) AS median,
  PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY salary) AS discrete_median,
  MODE() WITHIN GROUP (ORDER BY department) AS most_common_dept
FROM employees;
```

**Database Support:**
- PostgreSQL 9.4+ ✅
- Oracle 9i+ ✅

**Implementation Complexity:**
- **Parser:** Medium (2-3 weeks)
- **Planner:** Medium (3-4 weeks)
- **Optimizer:** Medium (3-4 weeks) - Requires sorted data or approximate algorithms

---

### 4.12 SEARCH and CYCLE Clauses (Recursive CTEs)

**Status:** ❌ Not Supported (basic recursive CTEs exist, but not SEARCH/CYCLE)

**Description:** Built-in cycle detection and search ordering for recursive queries.

**Syntax:**
```sql
WITH RECURSIVE ancestors AS (
  SELECT id, parent_id, name, 1 AS level
  FROM people WHERE id = 100

  UNION ALL

  SELECT p.id, p.parent_id, p.name, a.level + 1
  FROM people p JOIN ancestors a ON p.id = a.parent_id
)
SEARCH BREADTH FIRST BY id SET ordercol
CYCLE id SET is_cycle USING path;
```

**What it does:**
- SEARCH clause - Specify BFS or DFS ordering
- CYCLE clause - Automatic cycle detection
- Prevents infinite loops in recursive queries
- Outputs ordering column and cycle detection flag

**Database Support:**
- PostgreSQL 14+ ✅
- Oracle 11g+ ✅
- SQL Server ❌ (manual cycle detection)
- MySQL ❌

**Implementation Complexity:**
- **Parser:** Medium (3-4 weeks)
- **Planner:** High (6-8 weeks) - BFS/DFS strategies, cycle tracking
- **Optimizer:** Medium (4-5 weeks)

**Optimization Opportunities:**
1. **BFS vs DFS** - Choose based on query pattern
2. **Cycle detection optimization** - Use hash table or bloom filter
3. **Early termination** - Stop when cycle detected

---

### 4.13 TRIM Array (array trimming)

**Status:** ❌ Not Supported

**Description:** Remove elements from beginning or end of arrays.

**Syntax:**
```sql
SELECT TRIM_ARRAY(arr, 2) -- Remove last 2 elements
FROM arrays;
```

**Database Support:**
- PostgreSQL (via array slicing) ⚠️
- SQL Server ❌
- Oracle ❌

**Implementation Complexity:** Low (1-2 weeks)

---

### 4.14 Multi-Set Operations (MULTISET)

**Status:** ❌ Not Supported

**Description:** Operations on nested collections.

**Syntax:**
```sql
SELECT customer_id,
       MULTISET(SELECT order_id FROM orders WHERE customer_id = c.id) AS orders
FROM customers c;
```

**Database Support:**
- Oracle ✅
- PostgreSQL (via arrays) ⚠️

**Implementation Complexity:** High (10-12 weeks)

---

### 4.15 NORMALIZE (Unicode Normalization)

**Status:** ❌ Not Supported

**Description:** Normalize Unicode strings to NFC, NFD, NFKC, or NFKD forms.

**Syntax:**
```sql
SELECT NORMALIZE(name, 'NFC') FROM users;
```

**Database Support:**
- PostgreSQL 13+ ✅
- Oracle ✅

**Implementation Complexity:** Low (1 week) - Depends on external library

---

### 4.16 OVERLAY (String Replacement)

**Status:** ❌ Not Supported

**Description:** Replace substring at specific position.

**Syntax:**
```sql
SELECT OVERLAY(name PLACING 'XXX' FROM 5 FOR 3) FROM users;
-- Result: Replaces 3 characters starting at position 5 with 'XXX'
```

**Database Support:**
- PostgreSQL ✅
- Oracle (via REPLACE) ⚠️
- SQL Server (via STUFF) ⚠️

**Implementation Complexity:** Low (1 week)

---

### 4.17 POSITION with Optional Arguments

**Status:** ⚠️ Partial (basic POSITION likely supported)

**Description:** POSITION with FROM clause and occurrence count.

**Syntax:**
```sql
SELECT POSITION('world' IN 'hello world world' FROM 7) -- Returns 13
FROM dual;
```

**Database Support:**
- Oracle ✅
- PostgreSQL ⚠️ (no FROM clause)

**Implementation Complexity:** Low (1 week)

---

### 4.18 SESSION_USER, CURRENT_CATALOG, CURRENT_SCHEMA

**Status:** ❌ Not Supported (SQL standard session information functions)

**Description:** Standard SQL session information functions.

**Database Support:**
- PostgreSQL ✅
- Oracle ✅
- SQL Server ✅

**Implementation Complexity:** Low (1 week)

---

### 4.19 NORMALIZE_TIMESTAMP (Time Zone Handling)

**Status:** ❌ Not Supported

---

### 4.20 Width Bucket Functions (QUANTIZE)

**Status:** ❌ Not Supported

**Description:** Discretize continuous values into buckets.

**Syntax:**
```sql
SELECT WIDTH_BUCKET(salary, 30000, 100000, 10) AS salary_bucket
FROM employees;
```

**What it does:**
- Divide range into equal-width buckets
- Returns bucket number (1 to N)
- Used for histograms, bucketing

**Database Support:**
- PostgreSQL ✅
- Oracle ✅
- SQL Server ❌

**Implementation Complexity:** Low (1-2 weeks)

---

### 4.21 REGR_* Statistical Functions

**Status:** ❌ Not Supported

**Description:** Linear regression aggregate functions.

**Functions:**
- REGR_SLOPE, REGR_INTERCEPT, REGR_R2
- REGR_COUNT, REGR_AVGX, REGR_AVGY
- REGR_SXX, REGR_SYY, REGR_SXY
- COVAR_POP, COVAR_SAMP
- CORR (correlation coefficient)

**Syntax:**
```sql
SELECT
  REGR_SLOPE(sales, advertising) AS slope,
  REGR_INTERCEPT(sales, advertising) AS intercept,
  REGR_R2(sales, advertising) AS r_squared
FROM campaigns;
```

**Database Support:**
- PostgreSQL ✅
- Oracle ✅
- SQL Server ❌

**Implementation Complexity:** Medium (3-4 weeks) - Statistical computations

---

### 4.22 EVERY / BOOL_AND and ANY / BOOL_OR (Boolean Aggregates)

**Status:** ❌ Not Supported

**Description:** Boolean aggregate functions.

**Syntax:**
```sql
SELECT
  EVERY(is_valid) AS all_valid,      -- AND of all values
  BOOL_OR(is_flagged) AS any_flagged -- OR of all values
FROM records;
```

**Database Support:**
- PostgreSQL ✅
- MySQL (via BIT_AND, BIT_OR) ⚠️
- SQL Server ❌

**Implementation Complexity:** Low (1 week)

---

### 4.23 BIT_AND, BIT_OR, BIT_XOR (Bitwise Aggregates)

**Status:** ❌ Not Supported

**Description:** Bitwise aggregate functions.

**Database Support:**
- PostgreSQL ✅
- MySQL ✅
- Oracle ❌

**Implementation Complexity:** Low (1 week)

---

### 4.24 RANGE Types and Operations

**Status:** ❌ Not Supported

**Description:** Range data types (int4range, tsrange, etc.) with operators.

**Syntax:**
```sql
-- Range construction
SELECT int4range(10, 20) @> 15 AS contains;

-- Range operations
SELECT int4range(1, 10) && int4range(5, 15) AS overlaps;
SELECT int4range(1, 10) + int4range(10, 20) AS union;
```

**Database Support:**
- PostgreSQL 9.2+ ✅
- Oracle ❌
- SQL Server ❌

**Implementation Complexity:** High (8-10 weeks) - New type system

---

### 4.25 ORDINALITY (WITH ORDINALITY in UNNEST)

**Status:** ✅ Supported (with_ordinality flag exists in Unnest operator)

---

### 4.26 CORRESPONDING (Set Operations)

**Status:** ❌ Not Supported

**Description:** Match columns by name in UNION/INTERSECT/EXCEPT.

**Syntax:**
```sql
SELECT id, name, email FROM users
UNION CORRESPONDING
SELECT user_id AS id, full_name AS name, email_address AS email FROM customers;
```

**What it does:**
- Match columns by name instead of position
- BY clause to specify subset of columns
- More flexible than positional matching

**Database Support:**
- Oracle ✅
- IBM DB2 ✅
- PostgreSQL ❌
- SQL Server ❌

**Implementation Complexity:** Medium (3-4 weeks)

---

### 4.27 NATURAL Set Operations

**Status:** ❌ Not Supported

**Description:** UNION NATURAL, INTERSECT NATURAL, EXCEPT NATURAL.

**Database Support:** Very limited

**Implementation Complexity:** Low (1-2 weeks)

---

### 4.28 OFFSET in Window Functions

**Status:** ❌ Not Supported (LAG/LEAD support offset, but not general OFFSET clause)

**Description:** OFFSET clause in window frame specification.

**Implementation Complexity:** Low (1-2 weeks)

---

### 4.29 EXCLUDE in Window Frames

**Status:** ❌ Not Supported

**Description:** EXCLUDE clause to remove rows from window frame.

**Syntax:**
```sql
SELECT
  SUM(value) OVER (
    ORDER BY date
    ROWS BETWEEN 3 PRECEDING AND CURRENT ROW
    EXCLUDE CURRENT ROW
  )
FROM time_series;
```

**What it does:**
- EXCLUDE CURRENT ROW
- EXCLUDE GROUP (peer group)
- EXCLUDE TIES
- EXCLUDE NO OTHERS (default)

**Database Support:**
- PostgreSQL 11+ ✅
- Oracle ❌
- SQL Server ❌

**Implementation Complexity:** Medium (2-3 weeks)

---

### 4.30 Named Arguments (Function Calls)

**Status:** ❌ Not Supported

**Description:** Call functions with named parameters.

**Syntax:**
```sql
SELECT substring(str => 'hello', start => 2, length => 3);
```

**Database Support:**
- PostgreSQL ✅
- Oracle ✅

**Implementation Complexity:** Medium (3-4 weeks)

---

## 5. Summary Table: Implementation Priority

| Feature | SQL Standard | Complexity | Priority | Estimated Weeks |
|---------|-------------|------------|----------|----------------|
| **JSON_TABLE** | SQL:2016 | High | High | 20-25 |
| **GROUPING SETS / CUBE / ROLLUP** | SQL:2019 | Very High | High | 25-30 |
| **PIVOT / UNPIVOT** | Non-standard | High | High | 15-20 |
| **SQL/PGQ (Graph Queries)** | SQL:2023 | Very High | Medium | 40-50 |
| **Temporal Tables (FOR SYSTEM_TIME)** | SQL:2016 | Very High | Medium | 30-35 |
| **Polymorphic Table Functions** | SQL:2016 | Very High | Medium | 25-30 |
| **LATERAL Subqueries** | SQL:1999 | High | High | 20-25 |
| **Multi-Dimensional Arrays** | SQL:2019 | Very High | Low | 25-30 |
| **WINDOW Clause (Named Windows)** | SQL:2016 | Medium | Medium | 5-7 |
| **RESPECT/IGNORE NULLS** | SQL:2016 | Low | Medium | 3-4 |
| **JSON Functions (JSON_QUERY, JSON_VALUE, etc.)** | SQL:2016 | Low-Medium | High | 10-15 total |
| **LISTAGG ON OVERFLOW** | SQL:2016 | Medium | Low | 4-5 |
| **FILTER Clause** | SQL:2003 | Low | Medium | 3-4 |
| **WITH TIES** | Non-standard | Medium | Low | 5-6 |
| **TABLESAMPLE** | SQL:2003 | Medium | Medium | 6-8 |
| **SEARCH/CYCLE in Recursive CTEs** | SQL:1999 | High | Medium | 10-12 |
| **Hypothetical-Set Aggregates** | SQL:2003 | Medium | Low | 6-8 |
| **PERIOD Predicates** | SQL:2019 | Medium | Low | 8-10 |
| **EXCLUDE in Window Frames** | SQL:2011 | Medium | Low | 3-4 |
| **Statistical Aggregates (REGR_*)** | SQL:2003 | Medium | Low | 4-6 |

---

## 6. Optimization Opportunities by Feature

### High-Value Optimizations

1. **JSON_TABLE with Predicate Pushdown**
   - Push predicates into JSONPath filters
   - Use JSON indexes for extraction
   - Parallelize array unnesting
   - Estimated benefit: 3-10x for JSON-heavy queries

2. **GROUPING SETS Shared Computation**
   - Single scan for multiple grouping levels
   - Sort-based multi-level aggregation
   - Estimated benefit: N-1 table scans saved (N = number of grouping sets)

3. **LATERAL Decorrelation**
   - Convert to hash join when possible
   - Memoize repeated correlated subquery calls
   - Estimated benefit: 10-100x for large datasets

4. **Temporal Table History Pruning**
   - Partition pruning on time ranges
   - Temporal index scans
   - Estimated benefit: 10-50x for point-in-time queries

5. **Graph Query Index Selection**
   - Use adjacency list indexes
   - Bidirectional path search
   - Graph pruning before traversal
   - Estimated benefit: 100-1000x for graph analytics

### Medium-Value Optimizations

6. **PIVOT Rewrite Optimization**
   - Efficient CASE expression generation
   - Column pruning for unused pivoted columns

7. **Window Frame EXCLUDE Optimization**
   - Efficient frame boundary tracking

8. **Polymorphic Table Function Inlining**
   - Inline simple PTFs to eliminate overhead

### Low-Value Optimizations

9. **WITH TIES Early Termination**
   - Stop after finding all ties

10. **TABLESAMPLE Cardinality Adjustment**
    - Accurate estimates for sampled queries

---

## 7. Database Support Matrix

| Feature | PostgreSQL | Oracle | SQL Server | MySQL | Snowflake |
|---------|-----------|--------|------------|-------|-----------|
| JSON_TABLE | ⚠️ (17+) | ✅ 12c+ | ⚠️ (OPENJSON) | ✅ 8.0+ | ✅ |
| MATCH_RECOGNIZE | ❌ | ✅ 12c+ | ❌ | ❌ | ✅ |
| GROUPING SETS | ✅ 9.5+ | ✅ 9i+ | ✅ 2008+ | ✅ 8.0+ | ✅ |
| PIVOT/UNPIVOT | ⚠️ | ✅ 11g+ | ✅ 2005+ | ❌ | ✅ |
| Temporal Tables | ⚠️ | ✅ | ✅ 2016+ | ❌ | ✅ |
| SQL/PGQ | ⚠️ (AGE) | ✅ 23c | ✅ | ❌ | ❌ |
| LATERAL | ✅ 9.3+ | ✅ 12c+ | ✅ | ✅ 8.0+ | ✅ |
| Polymorphic Table Functions | ⚠️ | ✅ 18c+ | ❌ | ❌ | ✅ |
| WINDOW Named | ✅ 8.4+ | ✅ 11g+ | ✅ 2012+ | ✅ 8.0+ | ✅ |
| RESPECT/IGNORE NULLS | ❌ | ✅ 11g+ | ✅ 2012+ | ❌ | ✅ |
| FILTER Clause | ✅ 9.4+ | ❌ | ❌ | ❌ | ✅ |
| TABLESAMPLE | ✅ 9.5+ | ✅ | ✅ | ❌ | ✅ |

**Legend:**
- ✅ = Fully supported
- ⚠️ = Partial support or via extension
- ❌ = Not supported

---

## 8. Recommendations

### Phase 1: High Priority (6-12 months)

1. **JSON_TABLE** - Critical for JSON analytics workloads
2. **GROUPING SETS/CUBE/ROLLUP** - Essential for OLAP queries
3. **PIVOT/UNPIVOT** - Common in reporting applications
4. **LATERAL** - Enables advanced subquery patterns
5. **JSON Functions Suite** - Complete JSON support

### Phase 2: Medium Priority (12-18 months)

6. **Temporal Tables** - Time-travel queries for auditing
7. **Polymorphic Table Functions** - Advanced table transformations
8. **WINDOW Named** - Improved query readability
9. **FILTER Clause** - Cleaner aggregate syntax
10. **TABLESAMPLE** - Approximate query processing

### Phase 3: Low Priority (18-24 months)

11. **SQL/PGQ** - Graph query support
12. **Multi-Dimensional Arrays** - Scientific computing
13. **Statistical Aggregates** - Advanced analytics
14. **SEARCH/CYCLE** - Enhanced recursive queries
15. **Other minor features**

---

## 9. Testing Strategy

For each missing feature, comprehensive testing should include:

1. **Parser Tests**
   - Valid syntax acceptance
   - Invalid syntax rejection
   - Edge cases and error messages

2. **Semantic Tests**
   - Correct relational algebra translation
   - Type checking and inference
   - Ambiguity resolution

3. **Optimization Tests**
   - Rule application verification
   - Cost model accuracy
   - Plan comparison with baseline databases

4. **Execution Tests**
   - Correctness on diverse datasets
   - Edge cases (NULL, empty sets, duplicates)
   - Cross-database validation

5. **Performance Tests**
   - Scalability benchmarks
   - Regression detection
   - Optimization effectiveness measurement

---

## 10. Conclusion

The Ra optimizer has strong foundational support for core SQL features but is missing 45+ feature groups from modern SQL standards (SQL:2016, SQL:2019, SQL:2023). The highest-priority gaps are:

1. **JSON_TABLE** - Essential for modern JSON workloads
2. **GROUPING SETS/CUBE/ROLLUP** - Core OLAP functionality
3. **PIVOT/UNPIVOT** - Common reporting pattern
4. **LATERAL** - Advanced subquery optimization opportunities
5. **Complete JSON function suite** - Round out JSON support

Implementation of these features would significantly enhance Ra's SQL compliance and enable optimization of a wider range of real-world queries. The estimated total effort is **300-400 weeks** for all missing features, suggesting a multi-year roadmap with phased implementation based on user demand and optimization impact.

**Next Steps:**
1. Prioritize features based on user feedback and query workload analysis
2. Create RFC documents for top 5 priority features
3. Implement parser support as Phase 1 for each feature
4. Add optimization rules in Phase 2
5. Validate against existing database implementations
