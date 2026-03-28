# Microsoft SQL Server Features Not Supported by Ra Optimizer

**Date:** 2026-03-28
**Scope:** Comprehensive analysis of SQL Server-specific features with no current Ra optimizer support

This document identifies ALL SQL Server-specific features that the Ra query optimizer does not currently support, based on analysis of the Ra codebase and Microsoft SQL Server documentation.

---

## Table of Contents

1. [Index Features](#index-features)
2. [In-Memory OLTP (Hekaton)](#in-memory-oltp-hekaton)
3. [Temporal Tables](#temporal-tables)
4. [Graph Database Features](#graph-database-features)
5. [JSON Functions](#json-functions)
6. [XML Features](#xml-features)
7. [Full-Text Search](#full-text-search)
8. [Advanced SQL Language Features](#advanced-sql-language-features)
9. [Spatial Data Types](#spatial-data-types)
10. [Query Hints and Optimization](#query-hints-and-optimization)
11. [Partitioning Features](#partitioning-features)
12. [Integration Complexity Assessment](#integration-complexity-assessment)

---

## Index Features

### 1. Clustered Columnstore Indexes

**Status:** Partial support (recognized but not fully optimized)

**Description:**
Column-oriented indexes optimized for OLAP workloads. Store data in compressed column segments rather than rows. Ra recognizes columnstore indexes (see `/home/gburd/ws/ra/crates/ra-metadata/src/sqlserver.rs` lines 365-366) but lacks advanced optimization rules.

**Syntax:**
```sql
CREATE CLUSTERED COLUMNSTORE INDEX ix_cs ON sales;

-- Ordered columnstore (SQL Server 2019+)
CREATE CLUSTERED COLUMNSTORE INDEX ix_cs_ordered ON sales
ORDER (customer_id, order_date);
```

**Use Cases:**
- Data warehouse fact tables (10x compression, 10x query performance)
- Full table aggregations
- Wide tables with selective column access
- Real-time operational analytics

**Performance Characteristics:**
- Compression: 10x typical compression ratio
- Batch mode execution: 2-4x query performance improvement
- Segment elimination: Skip row groups that don't match predicates
- Ordered columnstore: Additional segment elimination for ordered columns
- Delta store: B-tree for small inserts before compression (102,400 row threshold)

**Ra Integration Complexity:** Medium-High
- Basic recognition exists in index metadata
- Missing: Batch mode execution modeling
- Missing: Segment elimination cost estimation
- Missing: Delta store vs. columnstore query routing
- Missing: Tuple-mover background process modeling
- Missing: Ordered columnstore optimization rules

**Optimization Strategies:**
1. Segment elimination based on min/max statistics per column segment
2. Batch mode execution for aggregations (process 64-900 rows at a time)
3. Pushdown filters to column segments
4. Dictionary-based string compression awareness
5. Ordered columnstore: exploit sort order for range predicates

---

### 2. Nonclustered Columnstore Indexes

**Status:** Partial support (recognized but not optimized)

**Description:**
Secondary columnstore index on rowstore table enabling real-time operational analytics. Allows concurrent OLTP on rowstore and analytics on columnstore.

**Syntax:**
```sql
CREATE NONCLUSTERED COLUMNSTORE INDEX ix_ncs
ON orders (order_date, amount, customer_id);

-- Filtered columnstore
CREATE NONCLUSTERED COLUMNSTORE INDEX ix_ncs_recent
ON orders (order_date, amount, customer_id)
WHERE order_date >= '2020-01-01';
```

**Use Cases:**
- Hybrid OLTP/OLAP workloads
- Reporting queries on transactional tables
- Avoiding ETL for near-real-time analytics

**Performance Characteristics:**
- Query performance: 10x improvement for analytic queries
- Storage: Separate copy of selected columns (compressed)
- Maintenance: Automatically updated on base table changes
- Concurrency: Read-only for queries, updated asynchronously

**Ra Integration Complexity:** Medium
- Needs dual-index cost modeling (rowstore vs. columnstore)
- Missing: Query routing logic (when to use columnstore vs. rowstore)
- Missing: Storage overhead estimation

**Optimization Strategies:**
1. Route aggregation queries to columnstore index
2. Route point lookups/single-row queries to rowstore
3. Consider filtered columnstore for hot data subsets
4. Model update overhead on base table

---

### 3. Filtered Indexes (Partial Indexes)

**Status:** Recognized but no filter-aware optimization

**Description:**
Indexes with WHERE clause that covers subset of table rows. Smaller, faster, and cheaper to maintain than full indexes.

**Syntax:**
```sql
-- SQL Server
CREATE INDEX ix_active ON users(email) WHERE active = 1;

-- PostgreSQL equivalent (already supported)
CREATE INDEX ix_active ON users(email) WHERE active = true;
```

**Use Cases:**
- Indexing common query subsets (e.g., active records only)
- Sparse column indexing (non-NULL values)
- Date range indexes (current year only)

**Performance Characteristics:**
- Size: 50-90% smaller than full index (depends on filter selectivity)
- Maintenance: Fewer rows to update
- Query: Only usable when query predicates match or imply filter

**Ra Integration Complexity:** Low-Medium
- Ra recognizes filtered indexes in metadata
- Missing: Filter clause parsing and matching
- Missing: Query predicate subsumption check
- Missing: Cost estimation for filtered vs. full index

**Optimization Strategies:**
1. Match query WHERE clause against index filter
2. Use filtered index when query predicates imply index filter
3. Prefer filtered index when estimated cardinality is low
4. Model smaller I/O cost due to reduced index size

---

### 4. Included Columns (Covering Indexes)

**Status:** Supported (see `/home/gburd/ws/ra/docs/features/index-types.md` lines 34-43)

**Description:**
Nonclustered B-tree index with additional non-key columns stored at leaf level. Enables index-only scans without heap/clustered index lookups.

**Syntax:**
```sql
CREATE INDEX ix_cust ON orders(customer_id) INCLUDE (order_date, total);
```

**Use Cases:**
- Queries that need filtering columns + additional retrieval columns
- Avoiding expensive key lookups
- Wide covering indexes without key-size limitations

**Performance Characteristics:**
- Eliminates heap/clustered index lookups
- Leaf-level storage only (not in B-tree navigation structure)
- No size limit on included columns (unlike key columns: 900-byte limit)

**Ra Integration Complexity:** Low
- Already modeled in Ra's `IndexType::NonClustered(Covering)`
- Covered column detection exists
- Cost model for index-only scan exists

---

### 5. XML Indexes

**Status:** Partial support for primary XML index; no secondary XML index support

**Description:**
Specialized indexes for XML data type. Primary index shreds XML into relational format; secondary indexes optimize specific XPath patterns.

**Syntax:**
```sql
-- Primary XML index (required first)
CREATE PRIMARY XML INDEX ix_xml_primary
ON documents(xml_column);

-- Secondary XML indexes
CREATE XML INDEX ix_xml_path ON documents(xml_column)
USING XML INDEX ix_xml_primary FOR PATH;

CREATE XML INDEX ix_xml_value ON documents(xml_column)
USING XML INDEX ix_xml_primary FOR VALUE;

CREATE XML INDEX ix_xml_property ON documents(xml_column)
USING XML INDEX ix_xml_primary FOR PROPERTY;
```

**Use Cases:**
- XPath/XQuery queries on XML columns
- `xml.exist()`, `xml.value()`, `xml.query()`, `xml.nodes()` methods
- XML document filtering and extraction

**Performance Characteristics:**
- Primary index: 3-4x storage overhead, shreds entire XML document
- PATH index: Optimizes `/doc/element` navigation
- VALUE index: Optimizes value-based predicates `[@price > 100]`
- PROPERTY index: Optimizes `xml.value()` extractions

**Ra Integration Complexity:** Medium-High
- Partial support in `/home/gburd/ws/ra/crates/ra-engine/src/xml_optimizer.rs`
- Missing: Secondary XML index types
- Missing: Cost estimation for XML index types
- Missing: XPath-to-index mapping

**Optimization Strategies:**
1. Route XPath navigation to PATH secondary index
2. Route value predicates to VALUE secondary index
3. Route property extraction to PROPERTY secondary index
4. Model XML shredding overhead
5. Estimate segment elimination for XML indexes (see xml_optimizer.rs lines 805-906)

---

### 6. Full-Text Indexes

**Status:** Recognized but no optimization rules

**Description:**
Inverted indexes for natural language search on text columns. Support linguistic word breaking, stemming, and relevance ranking.

**Syntax:**
```sql
-- Enable full-text catalog
CREATE FULLTEXT CATALOG ft_catalog AS DEFAULT;

-- Create full-text index
CREATE FULLTEXT INDEX ON articles(title, body)
KEY INDEX PK_articles
WITH CHANGE_TRACKING AUTO;
```

**Use Cases:**
- Document search (contains word/phrase)
- Linguistic search (stemming, thesaurus, language-specific)
- Relevance ranking
- Near searches (words within N words of each other)

**Performance Characteristics:**
- Index size: 20-40% of text data size
- Query: Sub-second search on millions of documents
- Maintenance: Asynchronous population (AUTO, MANUAL, or OFF)
- Ranking: CONTAINSTABLE returns relevance scores

**Ra Integration Complexity:** Medium
- Recognized in index metadata (`IndexType::FullText` exists)
- Missing: Full-text predicate recognition (CONTAINS, FREETEXT, CONTAINSTABLE, FREETEXTTABLE)
- Missing: Linguistic feature modeling
- Missing: Relevance ranking cost estimation

**Optimization Strategies:**
1. Detect CONTAINS/FREETEXT predicates
2. Route text search to full-text index
3. Model inverted index lookup cost
4. Consider index population lag for change tracking

---

## In-Memory OLTP (Hekaton)

**Status:** Minimal support (rule file exists but not integrated)

**Description:**
Memory-optimized tables and natively compiled stored procedures. Entire table resides in memory with lock-free data structures and optimistic concurrency control.

**Syntax:**
```sql
-- Memory-optimized table
CREATE TABLE orders_mem (
    order_id INT PRIMARY KEY NONCLUSTERED,
    customer_id INT NOT NULL,
    order_date DATETIME2 NOT NULL,
    total DECIMAL(10,2) NOT NULL
) WITH (MEMORY_OPTIMIZED = ON, DURABILITY = SCHEMA_AND_DATA);

-- Natively compiled stored procedure
CREATE PROCEDURE usp_insert_order
    @customer_id INT, @total DECIMAL(10,2)
WITH NATIVE_COMPILATION, SCHEMABINDING
AS BEGIN ATOMIC WITH (TRANSACTION ISOLATION LEVEL = SNAPSHOT, LANGUAGE = 'English')
    INSERT INTO orders_mem (customer_id, order_date, total)
    VALUES (@customer_id, GETDATE(), @total);
END;
```

**Use Cases:**
- High-throughput OLTP (30x performance gain reported)
- Low-latency transaction processing (<1ms latency)
- Session state caching (1.2M requests/sec achieved by bwin.com)
- Transient data (non-durable tables with DURABILITY = SCHEMA_ONLY)

**Performance Characteristics:**
- Memory access: 10-100x faster than disk
- Lock-free structures: No locking/latching overhead
- Optimistic concurrency: Validation at commit time
- Natively compiled procedures: Compiled to native code (5-10x faster)
- Durability: Optional (durable vs. non-durable tables)

**Ra Integration Complexity:** High
- Rule file exists: `/home/gburd/ws/ra/rules/database-specific/mssql/in-memory-oltp.rra`
- Missing: Memory-optimized table detection in query planner
- Missing: Concurrency control modeling (optimistic vs. pessimistic)
- Missing: Native compilation cost benefits
- Missing: Hash index optimization (memory-optimized hash indexes)
- Missing: Range index optimization (memory-optimized nonclustered indexes)

**Optimization Strategies:**
1. Route point lookups to hash indexes on memory-optimized tables
2. Route range scans to range indexes on memory-optimized tables
3. Model lock-free access cost (significantly lower than disk-based)
4. Consider natively compiled procedure overhead (compilation cost)
5. Route transient data to non-durable tables

**Limitations:**
- Table size: Limited by available memory
- Durability: Non-durable tables lose data on restart
- Triggers: Not supported on memory-optimized tables
- Foreign keys: Not supported on memory-optimized tables
- DML triggers: Supported with restrictions

---

## Temporal Tables

**Status:** Not supported

**Description:**
System-versioned tables that automatically track full history of data changes. Every update creates a history record with validity period timestamps.

**Syntax:**
```sql
CREATE TABLE employees (
    employee_id INT PRIMARY KEY,
    name NVARCHAR(100) NOT NULL,
    salary DECIMAL(10,2) NOT NULL,
    valid_from DATETIME2 GENERATED ALWAYS AS ROW START,
    valid_to DATETIME2 GENERATED ALWAYS AS ROW END,
    PERIOD FOR SYSTEM_TIME (valid_from, valid_to)
) WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = dbo.employees_history));

-- Query as of point in time
SELECT * FROM employees FOR SYSTEM_TIME AS OF '2024-01-15';

-- Query changes between dates
SELECT * FROM employees FOR SYSTEM_TIME BETWEEN '2024-01-01' AND '2024-12-31';

-- Query all history
SELECT * FROM employees FOR SYSTEM_TIME ALL;
```

**Use Cases:**
- Data auditing and compliance
- Point-in-time analysis (state of data at specific time)
- Slowly changing dimensions (SCD Type 2)
- Accidental change recovery
- Trend analysis over time

**Performance Characteristics:**
- Insert: Minimal overhead (set valid_from, valid_to = 9999-12-31)
- Update: Writes old version to history table, updates current table
- Delete: Moves row to history table with valid_to = transaction time
- Query: FOR SYSTEM_TIME filters rows based on validity period
- Storage: History table can grow unbounded (retention policy recommended)

**Ra Integration Complexity:** High
- Missing: Temporal table detection and metadata
- Missing: FOR SYSTEM_TIME clause parsing
- Missing: Period column recognition (valid_from, valid_to)
- Missing: History table join optimization
- Missing: Temporal query cost estimation
- Missing: Retention policy modeling

**Optimization Strategies:**
1. Detect FOR SYSTEM_TIME queries early in parsing
2. Route AS OF queries to current table when date >= current time
3. Use clustered index on valid_from in history table
4. Consider partition switching for history table archival
5. Model history table size in cost estimation
6. Optimize BETWEEN queries to minimize history table scan

**Limitations:**
- Cannot have triggers on temporal tables
- Cannot alter system period columns directly
- History table must have same schema as current table
- Foreign keys point to current table only (not history)

---

## Graph Database Features

**Status:** Not supported

**Description:**
Native graph data structures (NODE and EDGE tables) with MATCH clause for pattern matching queries. Expresses multi-hop navigation and transitive closure easily.

**Syntax:**
```sql
-- Node table
CREATE TABLE person (
    person_id INT PRIMARY KEY,
    name NVARCHAR(100)
) AS NODE;

-- Edge table
CREATE TABLE friends (
    since DATE
) AS EDGE;

-- Insert nodes
INSERT INTO person VALUES (1, 'Alice'), (2, 'Bob'), (3, 'Carol');

-- Insert edges
INSERT INTO friends VALUES ((SELECT $node_id FROM person WHERE person_id = 1),
                            (SELECT $node_id FROM person WHERE person_id = 2),
                            '2020-01-15');

-- Pattern matching query
SELECT p2.name AS friend_name
FROM person AS p1, friends, person AS p2
WHERE MATCH(p1-(friends)->p2)
  AND p1.name = 'Alice';

-- Multi-hop query (SQL Server 2019+)
SELECT DISTINCT p2.name
FROM person AS p1,
     friends FOR PATH AS f,
     person FOR PATH AS p,
     person AS p2
WHERE MATCH(SHORTEST_PATH(p1(-(f)->p)+p2))
  AND p1.name = 'Alice';
```

**Use Cases:**
- Social networks (friends, followers)
- Recommendation engines (users who bought this also bought)
- Knowledge graphs
- Fraud detection (relationship analysis)
- Hierarchical data (reporting structures, bill of materials)

**Performance Characteristics:**
- Pattern matching: Single query replaces multiple self-joins
- Multi-hop: SHORTEST_PATH finds transitive closure efficiently
- Edge constraints: Restrict node types at DDL time
- Pseudo-columns: $node_id, $edge_id, $from_id, $to_id automatically generated
- Storage: Relational tables under the hood (with graph metadata)

**Ra Integration Complexity:** Very High
- Missing: NODE/EDGE table detection
- Missing: MATCH clause parsing
- Missing: Graph pattern compilation to relational algebra
- Missing: Multi-hop query optimization
- Missing: SHORTEST_PATH function support
- Missing: Edge constraint validation
- Missing: Graph pseudo-column handling ($node_id, etc.)

**Optimization Strategies:**
1. Compile MATCH patterns to semi-joins or anti-joins
2. Use indexes on $from_id and $to_id for edge traversal
3. Optimize SHORTEST_PATH with bidirectional BFS
4. Materialize transitive closure for small graphs
5. Prune search space with edge constraints
6. Consider graph-specific indexes (node index, edge index)

**Limitations:**
- MATCH not supported in views, inline table-valued functions
- Cannot combine MATCH with PIVOT, UNPIVOT
- Recursive patterns limited to SHORTEST_PATH
- No graph-specific query hints

---

## JSON Functions

**Status:** Basic support exists; missing advanced functions

**Description:**
Native JSON data type support with functions to parse, query, modify, and generate JSON. Stored as nvarchar(max) with constraint checking.

**Functions:**

### Supported (in function catalog):
- `STRING_AGG` - aggregate to delimited string (can generate JSON arrays)

### Not Supported:
- `ISJSON()` - validate JSON syntax
- `JSON_VALUE()` - extract scalar value from JSON
- `JSON_QUERY()` - extract object or array from JSON
- `JSON_MODIFY()` - update JSON document
- `FOR JSON AUTO/PATH` - generate JSON from query results
- `OPENJSON()` - parse JSON into rowset
- `JSON_ARRAY()` - construct JSON array (SQL Server 2022+)
- `JSON_OBJECT()` - construct JSON object (SQL Server 2022+)
- `JSON_ARRAYAGG()` - aggregate to JSON array (SQL Server 2022+)
- `JSON_OBJECTAGG()` - aggregate to JSON object (SQL Server 2022+)
- `JSON_PATH_EXISTS()` - test if JSON path exists (SQL Server 2022+)

**Syntax:**
```sql
-- Validate JSON
SELECT ISJSON('{"name":"Alice","age":30}') AS is_valid;  -- Returns 1

-- Extract scalar value
SELECT JSON_VALUE('{"name":"Alice","age":30}', '$.name') AS name;  -- Returns 'Alice'

-- Extract object/array
SELECT JSON_QUERY('{"person":{"name":"Alice"}}', '$.person') AS person;

-- Modify JSON
SELECT JSON_MODIFY('{"name":"Alice","age":30}', '$.age', 31) AS updated;

-- Generate JSON from query
SELECT employee_id, name, salary
FROM employees
FOR JSON AUTO;

-- Parse JSON to rowset
SELECT * FROM OPENJSON('{"name":"Alice","age":30}')
WITH (name NVARCHAR(100), age INT);

-- Construct JSON array (SQL Server 2022+)
SELECT JSON_ARRAY('Alice', 'Bob', 'Carol') AS names;

-- Construct JSON object (SQL Server 2022+)
SELECT JSON_OBJECT('name': 'Alice', 'age': 30) AS person;
```

**Use Cases:**
- RESTful API data exchange
- Schema-less data storage
- Semi-structured data integration
- Configuration storage
- Logging and telemetry

**Performance Characteristics:**
- Storage: UTF-16 encoded text (2 bytes per char)
- Parsing: CPU-intensive (parse on every query)
- Indexing: Computed columns + indexes on extracted values
- FOR JSON: Streaming generation (low memory overhead)
- OPENJSON: Row-by-row parsing

**Ra Integration Complexity:** Medium
- Missing: JSON function recognition in expression evaluator
- Missing: JSON path expression parsing
- Missing: JSON schema validation
- Missing: Computed column index matching for JSON extracts
- Missing: FOR JSON clause handling in SELECT

**Optimization Strategies:**
1. Create computed columns for frequently accessed JSON paths
2. Index computed columns for filtering
3. Use ISJSON in CHECK constraint for validation at write time
4. Avoid parsing same JSON multiple times (materialize in CTE)
5. Use FOR JSON PATH for custom JSON structure
6. Consider JSON_MODIFY cost (rewrite entire document)

---

## XML Features

**Status:** Partial support (basic XPath parsing exists)

**Description:**
Native XML data type with XQuery/XPath querying, XML schema validation, and XML indexes. Ra has XPath optimizer (`/home/gburd/ws/ra/crates/ra-engine/src/xml_optimizer.rs`) but incomplete.

**Functions:**

### Partially Supported:
- `xpath()` - PostgreSQL XPath (some parsing exists)
- `xmlexists()` - PostgreSQL XML existence check

### Not Supported:
- `xml.value()` - SQL Server method to extract scalar
- `xml.query()` - SQL Server method to extract XML fragment
- `xml.exist()` - SQL Server method to test XPath existence
- `xml.nodes()` - SQL Server method to shred XML into rowset
- `xml.modify()` - SQL Server method to update XML
- `FOR XML AUTO/PATH/EXPLICIT/RAW` - generate XML from query
- `OPENXML()` - parse XML document into rowset
- XML schema collections (XSD validation)

**Syntax:**
```sql
-- SQL Server XML methods
DECLARE @x XML = '<doc><item price="100"/><item price="200"/></doc>';

-- Extract scalar value
SELECT @x.value('(/doc/item/@price)[1]', 'INT') AS first_price;  -- Returns 100

-- Extract XML fragment
SELECT @x.query('/doc/item[@price > 100]') AS expensive_items;

-- Test existence
SELECT @x.exist('/doc/item[@price > 150]') AS has_expensive;  -- Returns 1

-- Shred to rowset
SELECT item.value('@price', 'INT') AS price
FROM @x.nodes('/doc/item') AS T(item);

-- Generate XML from query
SELECT employee_id, name, salary
FROM employees
FOR XML PATH('employee'), ROOT('employees');

-- Parse XML to rowset
DECLARE @xml XML = '<root><row id="1" name="Alice"/></root>';
SELECT * FROM OPENXML(@xml, '/root/row')
WITH (id INT '@id', name NVARCHAR(100) '@name');
```

**Use Cases:**
- Legacy data exchange (SOAP, web services)
- Configuration storage
- Document storage with XPath querying
- XML schema validation
- Hierarchical data representation

**Performance Characteristics:**
- Storage: UTF-16 encoded, compressed (25-40% of text size)
- Parsing: Expensive (parse on every query)
- Indexing: Primary XML index (3-4x storage), secondary indexes (PATH, VALUE, PROPERTY)
- FOR XML: Streaming generation
- XQuery: Interpreted execution (slow for complex queries)

**Ra Integration Complexity:** Medium-High
- Partial support in xml_optimizer.rs (lines 1-2463)
- XPath parsing exists (parse_xpath function, line 490)
- Cost estimation exists (estimate_xpath_cost, line 940)
- Missing: SQL Server-specific methods (.value, .query, .exist, .nodes, .modify)
- Missing: FOR XML clause handling
- Missing: OPENXML support
- Missing: XML schema validation
- Missing: Secondary XML index optimization (lines 805-906 has framework)

**Optimization Strategies:**
1. Use primary XML index for all XPath queries
2. Use PATH secondary index for navigation (`/doc/item`)
3. Use VALUE secondary index for predicates (`[@price > 100]`)
4. Use PROPERTY secondary index for `.value()` extractions
5. Avoid redundant XML parsing (materialize in CTE)
6. Consider XML compression (enabled by default in SQL Server)

---

## Full-Text Search

**Status:** Recognized but no predicate optimization

**Description:**
Advanced text search with linguistic processing, relevance ranking, and thesaurus support. Uses inverted indexes with word breakers and stemmers.

**Functions:**
- `CONTAINS()` - word/phrase search with boolean operators
- `FREETEXT()` - natural language search with stemming
- `CONTAINSTABLE()` - returns ranked result set
- `FREETEXTTABLE()` - returns ranked result set with generated query
- Semantic search functions (SQL Server 2012+)

**Syntax:**
```sql
-- Word search
SELECT * FROM articles
WHERE CONTAINS(body, 'database');

-- Phrase search
SELECT * FROM articles
WHERE CONTAINS(body, '"query optimization"');

-- Boolean operators
SELECT * FROM articles
WHERE CONTAINS(body, 'database AND (optimization OR performance)');

-- Proximity search
SELECT * FROM articles
WHERE CONTAINS(body, 'NEAR((query, optimization), 5)');

-- Ranked results
SELECT article_id, title, KEY_TBL.RANK
FROM articles
INNER JOIN CONTAINSTABLE(articles, body, 'database') AS KEY_TBL
  ON articles.article_id = KEY_TBL.[KEY]
ORDER BY KEY_TBL.RANK DESC;

-- Free-text search (automatic stemming)
SELECT * FROM articles
WHERE FREETEXT(body, 'optimizing queries');  -- Matches "optimize", "optimized", etc.
```

**Use Cases:**
- Document search systems
- Content management systems
- E-commerce product search
- Knowledge bases
- Legal document discovery

**Performance Characteristics:**
- Index size: 20-40% of text data size
- Query: Sub-second on millions of documents
- Ranking: TF-IDF based relevance scores
- Maintenance: Asynchronous population (background process)
- Word breaking: Language-specific (50+ languages supported)

**Ra Integration Complexity:** High
- IndexType::FullText exists in index metadata
- Missing: CONTAINS/FREETEXT predicate recognition in parser
- Missing: Full-text query syntax parsing (AND/OR/NEAR operators)
- Missing: CONTAINSTABLE/FREETEXTTABLE table-valued function support
- Missing: Relevance ranking cost estimation
- Missing: Change tracking modeling (AUTO/MANUAL/OFF)
- Missing: Language-specific stemming modeling

**Optimization Strategies:**
1. Detect CONTAINS/FREETEXT predicates early
2. Route to full-text index instead of table scan
3. Model inverted index lookup cost (log N lookups + merge)
4. Consider index population lag (change tracking mode)
5. Use CONTAINSTABLE when ranking is needed
6. Pushdown non-full-text predicates to base table first
7. Model NEAR operator cost (positional index lookups)

---

## Advanced SQL Language Features

### 1. MERGE Statement

**Status:** Supported in parser; missing optimization rules

**Description:**
Atomic upsert operation combining INSERT, UPDATE, DELETE in single statement. Matches source to target and performs actions based on match status.

**Syntax:**
```sql
MERGE INTO target_table AS T
USING source_table AS S
ON T.key = S.key
WHEN MATCHED AND S.status = 'deleted' THEN
    DELETE
WHEN MATCHED THEN
    UPDATE SET T.value = S.value, T.updated_at = GETDATE()
WHEN NOT MATCHED BY TARGET THEN
    INSERT (key, value, created_at)
    VALUES (S.key, S.value, GETDATE())
WHEN NOT MATCHED BY SOURCE THEN
    DELETE
OUTPUT $action, INSERTED.*, DELETED.*;
```

**Use Cases:**
- Data warehouse ETL (synchronize staging to target)
- Change data capture (apply change log)
- Database synchronization
- Slowly changing dimensions (SCD Type 1/2)

**Performance Characteristics:**
- Atomic: All operations in single transaction
- Optimized joins: Single scan of target and source
- OUTPUT clause: Capture affected rows
- Index usage: Uses indexes on join columns
- Locking: Can cause deadlocks if not carefully designed

**Ra Integration Complexity:** Medium
- MERGE parsing likely exists (CHECK: ra-parser)
- Missing: MERGE-specific optimization rules
- Missing: Cost model for multi-action MERGE
- Missing: WHEN clause selectivity estimation
- Missing: OUTPUT clause integration with MERGE

**Optimization Strategies:**
1. Use covering index on join columns
2. Consider partitioning for large MERGE operations
3. Split complex MERGE into simpler MERGE statements
4. Use WHEN NOT MATCHED BY SOURCE carefully (full table scan)
5. Add index hints if optimizer chooses wrong index
6. Model OUTPUT clause overhead

**Limitations:**
- Cannot update same row twice in MERGE
- WHEN clauses evaluated in order
- Graph MATCH patterns supported (SQL Server 2019+)

---

### 2. OUTPUT Clause

**Status:** Not supported

**Description:**
Returns data from INSERT, UPDATE, DELETE, MERGE operations. Can capture modified rows to table variable or return to client.

**Syntax:**
```sql
-- Return deleted rows to client
DELETE FROM orders
OUTPUT DELETED.*
WHERE order_date < '2020-01-01';

-- Capture inserted rows to table variable
DECLARE @inserted_orders TABLE (order_id INT, customer_id INT, total DECIMAL(10,2));

INSERT INTO orders (customer_id, order_date, total)
OUTPUT INSERTED.order_id, INSERTED.customer_id, INSERTED.total INTO @inserted_orders
VALUES (123, GETDATE(), 99.99);

-- Capture before and after values in UPDATE
UPDATE employees
SET salary = salary * 1.10
OUTPUT DELETED.employee_id, DELETED.salary AS old_salary,
       INSERTED.salary AS new_salary
WHERE department = 'Engineering';

-- OUTPUT with MERGE
MERGE INTO inventory AS T
USING daily_sales AS S ON T.product_id = S.product_id
WHEN MATCHED THEN UPDATE SET T.quantity = T.quantity - S.quantity
OUTPUT $action, INSERTED.*, DELETED.*;
```

**Use Cases:**
- Audit logging (capture changes)
- Identity value retrieval after INSERT
- Archiving deleted data
- Change data capture
- Multi-step data processing

**Performance Characteristics:**
- Minimal overhead: Single pass through modified rows
- Memory: Table variables can use tempdb if large
- Locking: Same as underlying DML operation
- Parallelism: Serial plan if OUTPUT to client; parallel if OUTPUT INTO

**Ra Integration Complexity:** Medium
- Missing: OUTPUT clause parsing
- Missing: INSERTED/DELETED pseudo-table handling
- Missing: OUTPUT INTO table variable/table support
- Missing: $action variable support (MERGE only)

**Optimization Strategies:**
1. Use OUTPUT INTO for large result sets (avoids client buffer)
2. Prefer table variables for small result sets (<1000 rows)
3. Use temp tables for large result sets (>1000 rows)
4. Model OUTPUT overhead in cost estimation (minimal)
5. Consider OUTPUT with MERGE for complex ETL

---

### 3. Table-Valued Parameters

**Status:** Not supported

**Description:**
Pass table-structured data as parameter to stored procedure or function. Enables set-based operations instead of row-by-row calls.

**Syntax:**
```sql
-- Define table type
CREATE TYPE dbo.order_items_type AS TABLE (
    product_id INT,
    quantity INT,
    price DECIMAL(10,2)
);

-- Stored procedure accepting table-valued parameter
CREATE PROCEDURE usp_process_order
    @customer_id INT,
    @items dbo.order_items_type READONLY
AS
BEGIN
    INSERT INTO order_items (order_id, product_id, quantity, price)
    SELECT @order_id, product_id, quantity, price
    FROM @items;
END;

-- Call with table-valued parameter
DECLARE @items dbo.order_items_type;
INSERT INTO @items VALUES (101, 2, 19.99), (102, 1, 49.99);
EXEC usp_process_order @customer_id = 1, @items = @items;
```

**Use Cases:**
- Bulk operations from application
- Complex business logic with multiple rows
- Passing result sets between procedures
- Replacing cursor-based processing

**Performance Characteristics:**
- Network: Single round-trip vs. multiple round-trips
- Memory: Table variable in tempdb
- Read-only: Cannot modify parameter in procedure
- Optimization: Can have indexes (inline)

**Ra Integration Complexity:** High
- Missing: Table type definition parsing
- Missing: READONLY constraint handling
- Missing: Table-valued parameter detection in procedure calls
- Missing: Cost model for table-valued parameters vs. temporary tables

**Optimization Strategies:**
1. Add inline indexes to table type definition
2. Prefer table-valued parameters over XML/JSON for bulk data
3. Model network cost savings (single round-trip)
4. Consider memory overhead (table variable in tempdb)

---

### 4. PIVOT and UNPIVOT Operators

**Status:** Not supported

**Description:**
Transpose rows to columns (PIVOT) or columns to rows (UNPIVOT). Syntactic sugar for GROUP BY and UNION ALL patterns.

**Syntax:**
```sql
-- PIVOT: Convert rows to columns
SELECT *
FROM (
    SELECT product, month, sales
    FROM sales_data
) AS source
PIVOT (
    SUM(sales)
    FOR month IN ([Jan], [Feb], [Mar])
) AS pivot_table;

-- UNPIVOT: Convert columns to rows
SELECT product, month, sales
FROM sales_summary
UNPIVOT (
    sales FOR month IN ([Jan], [Feb], [Mar])
) AS unpivot_table;
```

**Use Cases:**
- Report formatting (crosstab reports)
- Data normalization (UNPIVOT)
- Data denormalization (PIVOT)
- Dynamic column generation

**Performance Characteristics:**
- PIVOT: Equivalent to GROUP BY with CASE expressions
- UNPIVOT: Equivalent to UNION ALL
- Optimization: Can use columnstore indexes effectively

**Ra Integration Complexity:** Medium
- Missing: PIVOT/UNPIVOT syntax parsing
- Missing: Conversion to GROUP BY (PIVOT) or UNION ALL (UNPIVOT)
- Missing: Column list expansion for dynamic PIVOT

**Optimization Strategies:**
1. Rewrite PIVOT as GROUP BY with CASE (optimizer can optimize)
2. Rewrite UNPIVOT as UNION ALL (more explicit)
3. Use columnstore indexes for PIVOT aggregations
4. Consider dynamic SQL for dynamic PIVOT columns

---

### 5. TRY_CONVERT, TRY_CAST, TRY_PARSE Functions

**Status:** Not supported

**Description:**
Safe conversion functions that return NULL instead of throwing error on conversion failure. Useful for data quality queries and ETL.

**Syntax:**
```sql
-- TRY_CONVERT: Returns NULL if conversion fails
SELECT TRY_CONVERT(INT, '123') AS valid_int,        -- Returns 123
       TRY_CONVERT(INT, 'abc') AS invalid_int;      -- Returns NULL

-- TRY_CAST: Returns NULL if cast fails
SELECT TRY_CAST('2024-01-15' AS DATE) AS valid_date,    -- Returns date
       TRY_CAST('invalid' AS DATE) AS invalid_date;     -- Returns NULL

-- TRY_PARSE: Culture-aware parsing, returns NULL if parse fails
SELECT TRY_PARSE('$1,234.56' AS MONEY USING 'en-US') AS amount;  -- Returns 1234.56
```

**Use Cases:**
- Data quality analysis (find conversion failures)
- ETL data cleansing
- Dynamic SQL with untrusted input
- Reporting with mixed data types

**Performance Characteristics:**
- Try-catch overhead: Minimal (no exception throwing)
- Null propagation: Standard SQL NULL handling
- Culture-specific: TRY_PARSE can be slower (locale processing)

**Ra Integration Complexity:** Low
- Missing: TRY_CONVERT/TRY_CAST/TRY_PARSE function recognition
- Missing: NULL return behavior modeling
- Missing: Culture parameter support (TRY_PARSE)

**Optimization Strategies:**
1. Use TRY_CONVERT for nullable type conversions
2. Filter NULL results for data quality checks
3. Model as regular CAST with NULL on error
4. Consider predicate pushdown with NULL checks

---

### 6. STRING_AGG and STRING_SPLIT Functions

**Status:** STRING_AGG supported; STRING_SPLIT not supported

**Description:**
- STRING_AGG: Aggregate string values with delimiter (like GROUP_CONCAT in MySQL)
- STRING_SPLIT: Split delimited string into table of values

**Syntax:**
```sql
-- STRING_AGG: Concatenate with delimiter
SELECT department, STRING_AGG(employee_name, ', ') AS employees
FROM employees
GROUP BY department;

-- STRING_AGG with ORDER BY (SQL Server 2017+)
SELECT department, STRING_AGG(employee_name, ', ') WITHIN GROUP (ORDER BY hire_date) AS employees
FROM employees
GROUP BY department;

-- STRING_SPLIT: Parse delimited string to table
SELECT value
FROM STRING_SPLIT('apple,banana,cherry', ',');

-- STRING_SPLIT with ordinal (SQL Server 2022+)
SELECT value, ordinal
FROM STRING_SPLIT('apple,banana,cherry', ',', 1);
```

**Use Cases:**
- Denormalizing for display (STRING_AGG)
- Parsing CSV data (STRING_SPLIT)
- Tag/keyword aggregation
- Comma-separated list generation

**Performance Characteristics:**
- STRING_AGG: Linear in number of rows
- STRING_SPLIT: Linear in string length
- Memory: Can cause memory grants if result is large

**Ra Integration Complexity:** Low
- STRING_AGG supported in function catalog
- Missing: STRING_SPLIT table-valued function
- Missing: WITHIN GROUP (ORDER BY) clause for STRING_AGG
- Missing: Ordinal parameter for STRING_SPLIT (SQL Server 2022+)

**Optimization Strategies:**
1. Use STRING_AGG with ORDER BY for deterministic results
2. Consider XML or JSON instead of STRING_SPLIT for complex data
3. Model memory grant for large STRING_AGG results
4. Pushdown filters before STRING_SPLIT for performance

---

## Spatial Data Types

**Status:** Basic support exists; missing SQL Server-specific features

**Description:**
Geometry and geography data types for spatial data. Support spatial operations, indexing, and coordinate systems. Ra has basic spatial support but missing SQL Server-specific methods.

**Data Types:**
- `geometry` - Planar (flat-earth) spatial data
- `geography` - Geodetic (round-earth) spatial data

**Methods:**

### Common Methods:
- `STGeometryType()` - Returns geometry type (Point, LineString, Polygon, etc.)
- `STX()`, `STY()` - Returns X/Y coordinates
- `STLength()` - Returns length of linear geometry
- `STArea()` - Returns area of polygonal geometry
- `STDistance()` - Returns distance between geometries
- `STIntersects()` - Tests if geometries intersect
- `STContains()` - Tests if geometry contains another
- `STWithin()` - Tests if geometry is within another
- `STBuffer()` - Returns buffered geometry
- `STIntersection()` - Returns intersection geometry
- `STUnion()` - Returns union geometry

### SQL Server-Specific:
- `STIsValid()` - Validates geometry (OGC compliance)
- `MakeValid()` - Repairs invalid geometry
- `Reduce()` - Simplifies geometry (Douglas-Peucker)
- `STNumCurves()` - Returns number of circular arcs
- `STCurveN()` - Returns Nth circular arc
- `STPointOnSurface()` - Returns point guaranteed inside polygon

**Syntax:**
```sql
-- Create geometry
DECLARE @point geometry = geometry::STGeomFromText('POINT(1 2)', 0);
DECLARE @polygon geometry = geometry::STGeomFromText('POLYGON((0 0, 10 0, 10 10, 0 10, 0 0))', 0);

-- Spatial operations
SELECT @point.STDistance(@polygon) AS distance;
SELECT @polygon.STContains(@point) AS contains;
SELECT @polygon.STArea() AS area;

-- Create geography (WGS84)
DECLARE @seattle geography = geography::Point(47.6062, -122.3321, 4326);
DECLARE @portland geography = geography::Point(45.5152, -122.6784, 4326);
SELECT @seattle.STDistance(@portland) AS distance_meters;  -- Returns meters

-- Spatial index
CREATE SPATIAL INDEX ix_geom ON parcels(geom)
WITH (BOUNDING_BOX = (xmin=0, ymin=0, xmax=100, ymax=100));
```

**Use Cases:**
- GIS applications (mapping, routing)
- Location-based services
- Geocoding and reverse geocoding
- Proximity search (nearest neighbor)
- Spatial analysis (buffer, overlay, intersection)

**Performance Characteristics:**
- Index: R-tree spatial index (4-level grid tessellation)
- Query: Bounding box filter + accurate geometry test (two-phase)
- Geography: Slower than geometry (geodetic calculations)
- Precision: Geometry uses planar math; geography uses geodetic

**Ra Integration Complexity:** Medium
- Basic spatial support exists (`DataType::Geometry` in function catalog)
- Missing: SQL Server-specific spatial methods
- Missing: Spatial index optimization (R-tree cost model)
- Missing: Geography vs. geometry differentiation
- Missing: Coordinate system (SRID) handling
- Missing: Two-phase filter cost estimation

**Optimization Strategies:**
1. Use spatial index for bounding box queries (Phase 1: fast filter)
2. Apply accurate geometry test only to candidates (Phase 2: exact test)
3. Use appropriate grid tessellation for data distribution
4. Prefer geometry over geography when data is localized (flat-earth OK)
5. Use STDistance() with index for K-nearest-neighbor queries
6. Model Phase 1 (index) vs. Phase 2 (exact test) cost separately

---

## Query Hints and Optimization

**Status:** Not supported

**Description:**
Query-level and table-level hints to override optimizer decisions. Used for performance tuning when optimizer chooses suboptimal plan.

### Query Hints (OPTION Clause):

**Syntax:**
```sql
SELECT * FROM orders
WHERE order_date >= '2024-01-01'
OPTION (RECOMPILE);

SELECT * FROM orders
WHERE order_date >= '2024-01-01'
OPTION (OPTIMIZE FOR (@date = '2024-06-15'));

SELECT * FROM orders
WHERE customer_id = @customer_id
OPTION (MAXDOP 4);

SELECT * FROM orders o
INNER JOIN customers c ON o.customer_id = c.customer_id
OPTION (HASH JOIN, MERGE JOIN);

SELECT * FROM orders
WHERE order_date >= '2024-01-01'
OPTION (USE PLAN N'<ShowPlanXML>...</ShowPlanXML>');
```

**Common Query Hints:**
- `RECOMPILE` - Recompile query each execution (no plan caching)
- `OPTIMIZE FOR` - Optimize for specific parameter value
- `OPTIMIZE FOR UNKNOWN` - Ignore parameter sniffing
- `MAXDOP N` - Limit degree of parallelism
- `HASH JOIN`, `MERGE JOIN`, `LOOP JOIN` - Force join algorithm
- `FAST N` - Optimize for first N rows
- `FORCE ORDER` - Preserve join order
- `USE PLAN` - Use specific execution plan (XML)
- `QUERYTRACEON` - Enable trace flag for query
- `MAX_GRANT_PERCENT` - Limit memory grant percentage
- `MIN_GRANT_PERCENT` - Minimum memory grant percentage

### Table Hints (WITH Clause):

**Syntax:**
```sql
SELECT * FROM orders WITH (NOLOCK)
WHERE order_date >= '2024-01-01';

SELECT * FROM orders WITH (INDEX(ix_order_date))
WHERE order_date >= '2024-01-01';

SELECT * FROM orders WITH (FORCESEEK)
WHERE customer_id = 123;

SELECT * FROM orders WITH (FORCESCAN)
WHERE order_date >= '2024-01-01';

SELECT * FROM orders WITH (READPAST)
WHERE status = 'pending';
```

**Common Table Hints:**
- `NOLOCK` (READ UNCOMMITTED) - Allow dirty reads
- `READCOMMITTED` - Read committed isolation (default)
- `REPEATABLEREAD` - Repeatable read isolation
- `SERIALIZABLE` - Serializable isolation
- `READPAST` - Skip locked rows
- `UPDLOCK` - Update lock
- `HOLDLOCK` - Hold shared locks until transaction ends
- `INDEX(index_name)` - Force specific index
- `FORCESEEK` - Force index seek (no scan)
- `FORCESCAN` - Force index/table scan (no seek)
- `TABLOCK` - Table-level lock
- `PAGLOCK` - Page-level lock
- `ROWLOCK` - Row-level lock

**Use Cases:**
- Parameter sniffing issues (OPTIMIZE FOR, RECOMPILE)
- Parallelism tuning (MAXDOP)
- Join order forcing (FORCE ORDER)
- Index forcing (INDEX hint)
- Isolation level override (NOLOCK, etc.)
- Lock granularity control (ROWLOCK, PAGLOCK, TABLOCK)

**Performance Characteristics:**
- Query hints: Can significantly improve or degrade performance
- Parameter sniffing: RECOMPILE prevents plan caching (CPU overhead)
- Parallelism: MAXDOP can reduce query time but increase CPU usage
- Table hints: Can bypass optimizer, leading to suboptimal plans

**Ra Integration Complexity:** High
- Missing: OPTION clause parsing
- Missing: WITH table hint parsing
- Missing: Hint enforcement in query planner
- Missing: Parameter sniffing detection and mitigation
- Missing: Plan cache integration (RECOMPILE semantics)

**Optimization Strategies:**
1. Use OPTION (RECOMPILE) for queries with highly variable parameters
2. Use OPTIMIZE FOR for known parameter distributions
3. Use MAXDOP to limit parallelism for OLTP queries
4. Use FORCE ORDER to override join order
5. Use INDEX hint only when optimizer consistently chooses wrong index
6. Use NOLOCK for read-only reporting queries (accept dirty reads)
7. Warn user when hints override optimizer (Ra could log warnings)

**Limitations:**
- Hints can become outdated as data changes
- Over-use of hints reduces optimizer effectiveness
- Some hints conflict (e.g., HASH JOIN + LOOP JOIN)
- USE PLAN requires exact XML format

---

## Partitioning Features

**Status:** Basic partitioning awareness; missing partition-specific optimizations

**Description:**
Horizontal table partitioning by range, list, or hash. Each partition can have independent storage, indexing, and maintenance. Enables partition elimination for queries.

**Syntax:**
```sql
-- Create partition function (range boundaries)
CREATE PARTITION FUNCTION pf_orders (DATE)
AS RANGE RIGHT FOR VALUES ('2023-01-01', '2023-04-01', '2023-07-01', '2023-10-01');

-- Create partition scheme (filegroup mapping)
CREATE PARTITION SCHEME ps_orders
AS PARTITION pf_orders
TO ([PRIMARY], [PRIMARY], [PRIMARY], [PRIMARY], [PRIMARY]);

-- Create partitioned table
CREATE TABLE orders (
    order_id INT,
    customer_id INT,
    order_date DATE,
    total DECIMAL(10,2)
) ON ps_orders(order_date);

-- Create partitioned index
CREATE INDEX ix_customer ON orders(customer_id)
ON ps_orders(order_date);  -- Aligned partitioning

-- Create non-aligned index
CREATE INDEX ix_total ON orders(total)
ON [PRIMARY];  -- All partitions on same filegroup
```

**Partition Maintenance:**
```sql
-- Split partition (add new boundary)
ALTER PARTITION FUNCTION pf_orders() SPLIT RANGE ('2024-01-01');

-- Merge partitions (remove boundary)
ALTER PARTITION FUNCTION pf_orders() MERGE RANGE ('2023-04-01');

-- Switch partition (fast data movement)
ALTER TABLE orders_staging SWITCH TO orders PARTITION 5;

-- Truncate partition
TRUNCATE TABLE orders WITH (PARTITIONS (1, 2));
```

**Use Cases:**
- Large tables (>100GB)
- Time-series data (partition by date)
- Data archival (switch out old partitions)
- Parallel maintenance (rebuild indexes per partition)
- Query performance (partition elimination)

**Performance Characteristics:**
- Partition elimination: Skip partitions that don't match query predicate (huge speedup)
- Parallel operations: Each partition can be processed in parallel
- Fast data movement: SWITCH operation is metadata-only (instant)
- Aligned indexes: Index partitions match table partitions (better performance)
- Non-aligned indexes: Index spans all partitions (slower maintenance)

**Ra Integration Complexity:** Medium-High
- Missing: Partition function and scheme parsing
- Missing: Partition elimination detection in query planner
- Missing: Cost model for partitioned vs. non-partitioned access
- Missing: Partition-wise join detection and cost estimation
- Missing: SWITCH operation optimization
- Missing: Parallel partition maintenance modeling

**Optimization Strategies:**
1. Detect partition elimination opportunities (WHERE clause on partition key)
2. Prefer aligned indexes for partitioned tables
3. Model partition-wise joins (each partition joined independently)
4. Estimate parallel partition scan cost (N partitions / parallel degree)
5. Consider SWITCH operation for bulk data movement
6. Model partition maintenance cost (REBUILD INDEX per partition)
7. Warn when query doesn't enable partition elimination

**Limitations:**
- Maximum 15,000 partitions per table
- Partition key must be part of clustered index key
- Cannot partition temporary tables
- Cannot partition tables with computed columns in partition key

---

## Integration Complexity Assessment

### Summary Table

| Feature Category | Integration Complexity | Priority | Estimated Effort |
|-----------------|----------------------|----------|------------------|
| Columnstore Indexes (Clustered) | Medium-High | High | 4-6 weeks |
| Columnstore Indexes (Nonclustered) | Medium | Medium | 2-3 weeks |
| Filtered Indexes | Low-Medium | High | 1-2 weeks |
| XML Indexes (Secondary) | Medium-High | Low | 3-4 weeks |
| Full-Text Indexes | Medium | Medium | 2-3 weeks |
| In-Memory OLTP | High | Low | 6-8 weeks |
| Temporal Tables | High | Medium | 4-6 weeks |
| Graph Database (MATCH) | Very High | Low | 8-12 weeks |
| JSON Functions | Medium | High | 2-4 weeks |
| XML Features (SQL Server) | Medium-High | Low | 3-4 weeks |
| Full-Text Search | High | Medium | 4-6 weeks |
| MERGE Statement | Medium | Medium | 2-3 weeks |
| OUTPUT Clause | Medium | Medium | 2-3 weeks |
| Table-Valued Parameters | High | Low | 4-6 weeks |
| PIVOT/UNPIVOT | Medium | Low | 2-3 weeks |
| TRY_CONVERT/CAST/PARSE | Low | High | 1 week |
| STRING_SPLIT | Low | High | 1 week |
| Spatial (SQL Server-specific) | Medium | Low | 2-3 weeks |
| Query Hints (OPTION) | High | Medium | 6-8 weeks |
| Table Hints (WITH) | High | Low | 4-6 weeks |
| Partitioning (Advanced) | Medium-High | Medium | 4-6 weeks |

### Priority Rationale:

**High Priority (Quick Wins):**
1. Filtered indexes - Widely used, low complexity
2. JSON functions - Modern applications, growing adoption
3. TRY_CONVERT/CAST/PARSE - Data quality, low complexity
4. STRING_SPLIT - Common ETL pattern, low complexity
5. Columnstore indexes (clustered) - OLAP performance, high impact

**Medium Priority (Important but Complex):**
1. Temporal tables - Auditing/compliance use case
2. Full-text search - Document-heavy applications
3. MERGE/OUTPUT - ETL workflows
4. Query hints - Performance tuning escape hatch
5. Partitioning - Large table performance

**Low Priority (Niche or Complex):**
1. In-Memory OLTP - Specialized workloads
2. Graph database - Niche use case, very complex
3. XML features - Legacy, declining usage
4. Table-valued parameters - Stored procedure specific
5. PIVOT/UNPIVOT - Can be rewritten with standard SQL

---

## Recommendations for Ra Integration

### Phase 1: Foundation (Quick Wins)
1. **Filtered Indexes** - Extend existing index metadata with filter clause
2. **TRY_CONVERT/CAST/PARSE** - Add to function catalog with NULL-on-error semantics
3. **STRING_SPLIT** - Add table-valued function support
4. **JSON Basic Functions** - Add ISJSON, JSON_VALUE, JSON_QUERY to function catalog

### Phase 2: OLAP Features (High Impact)
1. **Columnstore Index Optimization** - Batch mode execution, segment elimination
2. **Partitioning Advanced** - Partition elimination, partition-wise joins
3. **Full-Text Search** - CONTAINS/FREETEXT predicate optimization

### Phase 3: ETL & Data Management
1. **MERGE Statement** - Multi-action DML optimization
2. **OUTPUT Clause** - Capture modified rows
3. **Temporal Tables** - FOR SYSTEM_TIME query optimization

### Phase 4: Advanced Features (Specialized)
1. **Query Hints** - OPTION clause as escape hatch for optimizer
2. **In-Memory OLTP** - Memory-optimized table detection and optimization
3. **Graph Database** - MATCH clause pattern matching

### Phase 5: Legacy & Niche
1. **XML Secondary Indexes** - Extend existing XML optimizer
2. **Spatial SQL Server Methods** - Add to spatial function catalog
3. **Table-Valued Parameters** - Stored procedure optimization

---

## Testing Strategy

For each feature implementation:

1. **Syntax Parsing Tests** - Verify SQL syntax is correctly parsed
2. **Metadata Detection Tests** - Verify feature is detected in schema metadata
3. **Cost Estimation Tests** - Verify cost model produces reasonable estimates
4. **Optimization Rule Tests** - Verify optimization rules fire correctly
5. **Integration Tests** - End-to-end tests with real SQL Server
6. **Regression Tests** - Ensure existing functionality not broken

---

## References

### Microsoft Documentation
- [Columnstore Indexes Overview](https://learn.microsoft.com/en-us/sql/relational-databases/indexes/columnstore-indexes-overview)
- [In-Memory OLTP Overview](https://learn.microsoft.com/en-us/sql/relational-databases/in-memory-oltp/overview-and-usage-scenarios)
- [Temporal Tables](https://learn.microsoft.com/en-us/sql/relational-databases/tables/temporal-tables)
- [Graph Processing](https://learn.microsoft.com/en-us/sql/relational-databases/graphs/sql-graph-overview)
- [JSON Functions](https://learn.microsoft.com/en-us/sql/t-sql/functions/json-functions-transact-sql)
- [OUTPUT Clause](https://learn.microsoft.com/en-us/sql/t-sql/queries/output-clause-transact-sql)

### Ra Codebase References
- Index types: `/home/gburd/ws/ra/docs/features/index-types.md`
- SQL coverage: `/home/gburd/ws/ra/docs/features/sql-coverage.md`
- SQL Server connector: `/home/gburd/ws/ra/crates/ra-metadata/src/sqlserver.rs`
- XML optimizer: `/home/gburd/ws/ra/crates/ra-engine/src/xml_optimizer.rs`
- Function catalog: `/home/gburd/ws/ra/crates/ra-catalog/src/functions.rs`
- In-Memory OLTP rule: `/home/gburd/ws/ra/rules/database-specific/mssql/in-memory-oltp.rra`

---

**Document Version:** 1.0
**Last Updated:** 2026-03-28
**Prepared By:** Claude Code Agent
**Review Status:** Ready for review
