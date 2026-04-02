# RFC 0094: JSON_TABLE Optimization

- Start Date: 2026-03-28
- Author: Ra Research Team
- Status: Draft
- SQL Standard: SQL:2016 (SQL/JSON)
- Tracking Issue: TBD

## Summary

Implement support for JSON_TABLE, a SQL:2016 standard feature that converts JSON data into relational table format. JSON_TABLE is critical for JSON analytics workloads and is supported by all major databases (PostgreSQL 17+, MySQL 8.0+, Oracle 12c+, SQL Server 2016+, Snowflake, Databricks). This RFC proposes parser extensions, relational algebra representation, and optimization rules to achieve 3-10x speedups for JSON queries through predicate pushdown, index usage, and parallel unnesting.

## Motivation

JSON has become ubiquitous in modern databases for storing semi-structured data (API responses, user preferences, event logs, product attributes). While databases provide JSON storage and basic extraction functions, converting JSON arrays and nested objects into queryable relational format remains expensive and poorly optimized.

### Current Limitations

Without JSON_TABLE support, developers must use verbose nested subqueries or lateral joins:

```sql
-- PostgreSQL: Verbose lateral join with jsonb_array_elements
SELECT o.order_id, item.value-&gt;&gt;'id' AS item_id,
       (item.value-&gt;&gt;'qty')::int AS quantity
FROM orders o,
     LATERAL jsonb_array_elements(o.order_items) AS item;

-- MySQL: JSON_EXTRACT with awkward array indexing
SELECT o.order_id,
       JSON_UNQUOTE(JSON_EXTRACT(o.order_items, CONCAT('$[', n.n, '].id'))) AS item_id,
       JSON_EXTRACT(o.order_items, CONCAT('$[', n.n, '].qty')) AS quantity
FROM orders o
CROSS JOIN (SELECT 0 AS n UNION ALL SELECT 1 UNION ALL SELECT 2 ...) n
WHERE JSON_EXTRACT(o.order_items, CONCAT('$[', n.n, ']')) IS NOT NULL;
```

These patterns are:
1. Verbose and error-prone
2. Difficult for optimizers to recognize and optimize
3. Force full JSON document parsing even for filtered results
4. Cannot leverage JSON-specific indexes effectively

### JSON_TABLE Benefits

JSON_TABLE provides a declarative syntax for JSON-to-relational conversion:

```sql
SELECT o.order_id, jt.item_id, jt.quantity
FROM orders o,
     JSON_TABLE(
       o.order_items,
       '$[*]' COLUMNS(
         item_id VARCHAR(50) PATH '$.id',
         quantity INT PATH '$.qty',
         price DECIMAL(10,2) PATH '$.price'
       )
     ) AS jt
WHERE jt.price &gt; 100;
```

**Advantages:**
- Standard SQL:2016 syntax across all major databases
- Declarative pattern enables aggressive optimization
- Type-safe column definitions with error handling
- Supports nested JSON structures with NESTED PATH
- Enables predicate pushdown into JSONPath expressions

### Database Support Matrix

| Database | Version | JSON_TABLE Support | JSON Index Type | Notes |
|----------|---------|-------------------|-----------------|-------|
| **PostgreSQL** | 17+ | ✅ (proposed) | GIN (JSONB) | jsonb_path_query optimization |
| **MySQL** | 8.0+ | ✅ | Multi-valued indexes | Binary JSON storage |
| **Oracle** | 12c+ | ✅ | JSON search indexes | Full SQL:2016 compliance |
| **SQL Server** | 2016+ | ✅ (as OPENJSON) | Computed column indexes | Uses FOR JSON syntax |
| **MariaDB** | 10.6+ | ✅ | Expression indexes | Compatible with MySQL |
| **Snowflake** | All | ✅ | VARIANT type | Columnar JSON storage |
| **Databricks** | All | ✅ | Delta column stats | Parquet-based optimization |

**Coverage:** 8 out of 8 major databases support JSON_TABLE or equivalent functionality.

### Expected Performance Impact

Based on analysis from SQL_STANDARDS_GAP_ANALYSIS.md:

| Query Pattern | Without Optimization | With Optimization | Speedup |
|---------------|---------------------|-------------------|---------|
| Simple array unnesting | Full parse + filter | Index scan + parallel unnest | 3-5x |
| Filtered array elements | Parse all + filter | JSONPath predicate pushdown | 5-10x |
| Nested JSON structures | Multiple parse passes | Single-pass extraction | 4-8x |
| Large JSON arrays (1000+ elements) | Sequential unnesting | Parallel worker unnesting | 8-15x |
| JSON with GIN/multi-valued indexes | Table scan + parse | Index-only scan | 10-50x |

**Overall impact:** 3-10x optimization for JSON analytics queries, with potential for 50x when indexes are available.

## Guide-level explanation

### Basic JSON_TABLE Syntax

Convert a JSON array into rows:

```sql
-- Input JSON: [{"id": 1, "name": "Item A"}, {"id": 2, "name": "Item B"}]
SELECT *
FROM JSON_TABLE(
  '[{"id": 1, "name": "Item A"}, {"id": 2, "name": "Item B"}]',
  '$[*]' COLUMNS(
    item_id INT PATH '$.id',
    item_name VARCHAR(100) PATH '$.name'
  )
) AS jt;

-- Result:
-- item_id | item_name
-- --------+----------
--       1 | Item A
--       2 | Item B
```

### Column Type Specifications

JSON_TABLE supports explicit type conversion:

```sql
SELECT *
FROM JSON_TABLE(
  '{"total": "123.45", "date": "2024-01-01", "active": "true"}',
  '$' COLUMNS(
    total DECIMAL(10,2) PATH '$.total',        -- String to decimal
    order_date DATE PATH '$.date',             -- String to date
    is_active BOOLEAN PATH '$.active',         -- String to boolean
    missing_col INT PATH '$.missing' DEFAULT 0 -- Default for missing
  )
);
```

### Nested JSON Structures

Use NESTED PATH to handle hierarchical data:

```sql
SELECT *
FROM orders o,
     JSON_TABLE(
       o.order_data,
       '$' COLUMNS(
         order_id INT PATH '$.id',
         customer VARCHAR(100) PATH '$.customer',
         NESTED PATH '$.items[*]' COLUMNS(
           item_id INT PATH '$.id',
           product VARCHAR(100) PATH '$.name',
           quantity INT PATH '$.qty',
           NESTED PATH '$.specs[*]' COLUMNS(
             spec_name VARCHAR(50) PATH '$.name',
             spec_value VARCHAR(100) PATH '$.value'
           )
         )
       )
     ) AS jt;
```

**Result structure:** Each nested array creates a cross-product, similar to nested lateral joins.

### Error Handling

JSON_TABLE provides error handling clauses:

```sql
SELECT *
FROM JSON_TABLE(
  '{"items": [{"price": "invalid"}, {"price": "123.45"}]}',
  '$.items[*]' COLUMNS(
    price DECIMAL(10,2) PATH '$.price'
      ERROR ON ERROR    -- Raise error on type conversion failure
      -- OR --
      NULL ON ERROR     -- Return NULL on error (default)
      -- OR --
      DEFAULT 0 ON ERROR -- Use default value on error
  )
);
```

**ON EMPTY clause:** Handle missing JSON paths
```sql
COLUMNS(
  optional_field VARCHAR(50) PATH '$.maybe_missing'
    NULL ON EMPTY          -- Return NULL if path doesn't exist
    DEFAULT 'N/A' ON EMPTY -- Use default if path doesn't exist
)
```

### Ordinal Columns

Track array position with FOR ORDINALITY:

```sql
SELECT *
FROM JSON_TABLE(
  '[{"name": "A"}, {"name": "B"}, {"name": "C"}]',
  '$[*]' COLUMNS(
    row_num FOR ORDINALITY,  -- 1, 2, 3, ...
    item_name VARCHAR(50) PATH '$.name'
  )
);

-- Result:
-- row_num | item_name
-- --------+----------
--       1 | A
--       2 | B
--       3 | C
```

## Reference-level explanation

### Grammar Extensions

Extend SQL parser with JSON_TABLE syntax:

```
table_reference ::=
  | JSON_TABLE (
      json_expr,
      json_path_expr
      COLUMNS ( column_definition [, column_definition]* )
    ) [AS] alias

column_definition ::=
  column_name type PATH json_path_expr [on_error] [on_empty]
  | column_name FOR ORDINALITY
  | NESTED [PATH] json_path_expr COLUMNS ( column_definition [, ...]* )

on_error ::=
  NULL ON ERROR
  | ERROR ON ERROR
  | DEFAULT literal ON ERROR

on_empty ::=
  NULL ON EMPTY
  | ERROR ON EMPTY
  | DEFAULT literal ON EMPTY

json_path_expr ::=
  string_literal  -- JSONPath expression (e.g., '$.items[*]')
```

**JSONPath syntax support:**
- `$` - Root object
- `.field` - Object field access
- `[*]` - Array wildcard (all elements)
- `[n]` - Array index (zero-based)
- `[start:end]` - Array slice
- `?(@.field &gt; value)` - Filter expressions

### Relational Algebra Representation

Introduce a new RelExpr variant:

```rust
pub enum RelExpr {
    // ... existing variants ...

    /// JSON_TABLE: Convert JSON to relational table
    JsonTable {
        /// JSON expression (column reference or literal)
        json_expr: Box&lt;Expr&gt;,

        /// JSONPath to array/object to unnest
        path_expr: String,

        /// Column definitions (name, type, path, error handling)
        columns: Vec&lt;JsonTableColumn&gt;,

        /// Statistics for cost estimation
        stats: Option&lt;JsonTableStats&gt;,
    },
}

pub struct JsonTableColumn {
    pub name: String,
    pub column_type: JsonTableColumnType,
    pub json_path: Option&lt;String&gt;,
    pub on_error: ErrorHandling,
    pub on_empty: ErrorHandling,
}

pub enum JsonTableColumnType {
    Ordinality,
    Regular { data_type: DataType },
    Nested {
        path: String,
        columns: Vec&lt;JsonTableColumn&gt;,
    },
}

pub enum ErrorHandling {
    Null,
    Error,
    Default(Const),
}

pub struct JsonTableStats {
    /// Estimated number of rows per input row
    pub avg_rows_per_input: f64,

    /// Average JSON document size in bytes
    pub avg_document_bytes: u64,

    /// Selectivity of JSONPath filters (if any)
    pub path_selectivity: f64,
}
```

### Optimization Rules

#### Rule 1: JSONPath Predicate Pushdown

Push WHERE clause predicates into the JSONPath expression:

```
Before:
  σ[jt.price &gt; 100](
    JsonTable(doc, '$[*]' COLUMNS(price PATH '$.price'))
  )

After:
  JsonTable(doc, '$[?(@.price &gt; 100)]' COLUMNS(price PATH '$.price'))
```

**Benefit:** JSON parser can filter array elements during traversal, avoiding materialization of filtered-out rows.

**Implementation:**
```rust
fn json_table_predicate_pushdown(
    filter: RelExpr,
    json_table: RelExpr,
) -&gt; Option&lt;RelExpr&gt; {
    // Extract predicates on JSON_TABLE columns
    let predicates = extract_json_table_predicates(&filter);

    // Convert to JSONPath filter expressions
    let jsonpath_filter = predicates_to_jsonpath_filter(predicates);

    // Merge into existing JSONPath expression
    let new_path = merge_jsonpath_filters(
        json_table.path_expr,
        jsonpath_filter
    );

    Some(JsonTable {
        path_expr: new_path,
        ..json_table
    })
}
```

#### Rule 2: JSON Index Scan

Use JSON-specific indexes when available:

```
Before:
  σ[jt.status = 'active'](
    JsonTable(doc, '$.items[*]' COLUMNS(status PATH '$.status'))
  )

After (PostgreSQL JSONB + GIN index):
  IndexScan(
    table,
    index: GIN(doc),
    condition: doc @&gt; '{"items": [{"status": "active"}]}'
  )
```

**Benefit:** 10-50x speedup using JSON-specific indexes instead of full document scan.

**Database-specific implementations:**
- **PostgreSQL:** GIN index with jsonb_path_query
- **MySQL 8.0:** Multi-valued indexes on JSON arrays
- **Oracle:** JSON search indexes
- **SQL Server:** Computed column indexes on JSON_VALUE expressions

#### Rule 3: Parallel JSON Array Unnesting

Partition large JSON arrays across parallel workers:

```
Before:
  JsonTable(doc, '$.items[*]' COLUMNS(...))
  -- Sequential unnesting of 10,000 array elements

After:
  ParallelUnion(
    JsonTable(doc, '$.items[0:2500]' COLUMNS(...)),   -- Worker 1
    JsonTable(doc, '$.items[2500:5000]' COLUMNS(...)), -- Worker 2
    JsonTable(doc, '$.items[5000:7500]' COLUMNS(...)), -- Worker 3
    JsonTable(doc, '$.items[7500:10000]' COLUMNS(...)) -- Worker 4
  )
```

**Benefit:** Near-linear speedup for large arrays (8-15x with 16 workers).

**Heuristic:** Apply when:
- JSON array has &gt;1000 elements (estimated from statistics)
- JSON_TABLE has no NESTED PATH (no dependencies between rows)
- Parallel workers available

#### Rule 4: Column Pruning

Skip unused JSON fields during extraction:

```
Before:
  π[item_id](
    JsonTable(doc, '$[*]' COLUMNS(
      item_id PATH '$.id',
      name PATH '$.name',           -- Unused
      description PATH '$.desc',    -- Unused
      price PATH '$.price'          -- Unused
    ))
  )

After:
  JsonTable(doc, '$[*]' COLUMNS(
    item_id PATH '$.id'
    -- Only extract used columns
  ))
```

**Benefit:** 2-4x speedup by skipping extraction and type conversion for unused columns.

#### Rule 5: Late Materialization

Defer JSON parsing until after filtering:

```
Before:
  σ[order_id = 123](
    orders ⋈ JsonTable(orders.items, '$[*]' COLUMNS(...))
  )

After:
  σ[order_id = 123](orders) ⋈ JsonTable(orders.items, '$[*]' COLUMNS(...))
```

**Benefit:** Parse JSON only for rows that pass early filters, avoiding wasted work.

#### Rule 6: NESTED PATH Flattening

Optimize nested JSON structures to avoid redundant parsing:

```
Before (two-pass):
  JsonTable(doc, '$' COLUMNS(
    order_id PATH '$.id',
    NESTED PATH '$.items[*]' COLUMNS(item_id PATH '$.id')
  ))

After (single-pass):
  JsonTable(doc, '$.items[*]' COLUMNS(
    order_id PATH '$.id' PASSING PARENT,  -- Access parent context
    item_id PATH '$.id'
  ))
```

**Benefit:** Single parse pass reduces overhead by 30-50%.

### Cost Model

JSON_TABLE cost depends on multiple factors:

```
json_table_cost = parse_cost + unnest_cost + extract_cost + filter_cost

parse_cost = avg_document_bytes * PARSE_COST_PER_BYTE
  where PARSE_COST_PER_BYTE = 0.001 (binary JSON) to 0.01 (text JSON)

unnest_cost = avg_rows_per_input * ROW_MATERIALIZE_COST
  where ROW_MATERIALIZE_COST = 0.5

extract_cost = num_columns * avg_rows_per_input * EXTRACT_COST_PER_FIELD
  where EXTRACT_COST_PER_FIELD = 0.1 (indexed path) to 1.0 (deep traversal)

filter_cost = avg_rows_per_input * num_predicates * PREDICATE_EVAL_COST
  where PREDICATE_EVAL_COST = 0.5 (simple) to 5.0 (complex)
```

**Selectivity estimation:**
```
-- JSONPath filter selectivity (heuristic)
path_selectivity = CASE
  WHEN filter = equality ($.field = value) THEN 0.01
  WHEN filter = range ($.field &gt; value) THEN 0.3
  WHEN filter = pattern ($.field LIKE '%value%') THEN 0.1
  ELSE 0.5  -- Unknown
END

-- Adjust for JSON array size
avg_rows_per_input = json_array_cardinality * path_selectivity
```

**JSON index cost:**
```
json_index_scan_cost = BASE_INDEX_COST +
  (estimated_matches * INDEX_TUPLE_COST)

-- Much cheaper than full document parse
json_index_scan_cost &lt;&lt; json_table_cost (when applicable)
```

### Cross-Database Translation

Different databases use different syntax:

**PostgreSQL 17+ (SQL:2016 standard):**
```sql
SELECT * FROM JSON_TABLE(
  data, '$.items[*]'
  COLUMNS(id INT PATH '$.id')
);
```

**MySQL 8.0 (SQL:2016 standard):**
```sql
SELECT * FROM JSON_TABLE(
  data, '$.items[*]'
  COLUMNS(id INT PATH '$.id')
) AS jt;
```

**SQL Server (OPENJSON):**
```sql
SELECT id
FROM OPENJSON(data, '$.items')
WITH (id INT '$.id');
```

**Oracle 12c+ (SQL:2016 standard):**
```sql
SELECT jt.*
FROM table_name t,
     JSON_TABLE(t.data, '$.items[*]'
       COLUMNS(id NUMBER PATH '$.id')
     ) jt;
```

**Snowflake (FLATTEN + LATERAL):**
```sql
SELECT f.value:id::INT AS id
FROM table_name t,
     LATERAL FLATTEN(input =&gt; t.data:items) f;
```

Ra's dialect layer will translate the canonical JSON_TABLE representation to database-specific syntax.

## Implementation Plan

### Phase 1: Parser Support (3-4 weeks)

**Tasks:**
1. Extend sqlparser-rs with JSON_TABLE grammar
   - JSON_TABLE keyword and syntax
   - COLUMNS clause parsing
   - NESTED PATH syntax
   - ON ERROR / ON EMPTY clauses
   - FOR ORDINALITY
2. Add AST nodes for JSON_TABLE
3. Write parser tests (50+ test cases)

**Deliverables:**
- `sqlparser::ast::JsonTable` struct
- Parser tests covering all syntax variants
- Error messages for malformed JSON_TABLE

### Phase 2: Relational Algebra Translation (2-3 weeks)

**Tasks:**
1. Add `RelExpr::JsonTable` variant
2. Implement SQL AST to RelExpr translation
3. Handle nested column definitions
4. Validate column types and error handling modes
5. Add unit tests for translation

**Deliverables:**
- `ra-core::RelExpr::JsonTable`
- Translation from sqlparser AST
- Type checking and validation

### Phase 3: Optimization Rules (4-5 weeks)

**Tasks:**
1. Implement predicate pushdown rule
2. Implement JSON index scan rule (PostgreSQL GIN, MySQL multi-valued)
3. Implement parallel unnesting rule
4. Implement column pruning rule
5. Implement late materialization rule
6. Implement NESTED PATH optimization
7. Add integration tests for each rule

**Deliverables:**
- 6 optimization rules in egg rewrite system
- Rule ordering and applicability conditions
- Integration tests with sample JSON data

### Phase 4: Cost Model (2-3 weeks)

**Tasks:**
1. Implement JSON_TABLE cost function
2. Add statistics collection for JSON columns
   - Average document size
   - Average array cardinality
   - Path selectivity estimation
3. Calibrate cost parameters
4. Add cardinality estimation tests

**Deliverables:**
- Cost model implementation
- Statistics gathering from database metadata
- Cost estimation tests

### Phase 5: Dialect Translation (2 weeks)

**Tasks:**
1. Implement JSON_TABLE to OPENJSON (SQL Server)
2. Implement JSON_TABLE to FLATTEN (Snowflake)
3. Handle dialect-specific quirks
4. Add cross-database translation tests

**Deliverables:**
- Dialect-specific translators
- Cross-database compatibility tests

### Phase 6: Testing and Benchmarking (2-3 weeks)

**Tasks:**
1. Write comprehensive integration tests
2. Test with real-world JSON datasets
3. Performance benchmarking:
   - Simple array unnesting
   - Nested JSON structures
   - Large arrays (1K, 10K, 100K elements)
   - With and without indexes
4. Compare performance vs. database native execution
5. Document performance characteristics

**Deliverables:**
- 100+ integration tests
- Performance benchmark suite
- Benchmark report comparing optimization effectiveness

**Total estimated effort: 17-21 weeks (4-5 months)**

## Testing Strategy

### Unit Tests

**Parser tests (50+ cases):**
- Valid JSON_TABLE syntax variations
- Column type specifications
- NESTED PATH syntax
- Error handling clauses
- Invalid syntax rejection
- Edge cases (empty COLUMNS, missing paths)

**Translation tests (30+ cases):**
- AST to RelExpr conversion
- Type inference
- Error handling mode validation
- Nested structure flattening

**Optimization rule tests (40+ cases):**
- Predicate pushdown correctness
- Column pruning correctness
- Cost comparison before/after optimization
- Rule applicability conditions

### Integration Tests

**Functional correctness (60+ cases):**
- Simple array unnesting
- Nested JSON with NESTED PATH
- Type conversion (string to int/decimal/date)
- Error handling (NULL/ERROR/DEFAULT on error/empty)
- FOR ORDINALITY column
- Filter predicates on JSON columns
- Join with JSON_TABLE
- Multiple JSON_TABLE in same query

**Cross-database compatibility (40+ cases):**
- Test same query on PostgreSQL, MySQL, SQL Server, Oracle
- Verify semantic equivalence of dialect translations
- Handle database-specific features gracefully

### Performance Tests

**Scalability benchmarks:**
- Small JSON (10 elements): baseline performance
- Medium JSON (1,000 elements): parallel unnesting activation
- Large JSON (100,000 elements): memory and parallelism limits
- Nested JSON (3 levels deep): overhead of nested processing

**Optimization effectiveness:**
- Measure speedup with each optimization rule
- Compare against baseline (no optimization)
- Compare against database native execution
- Target: 3-10x speedup for typical JSON queries

**Index usage:**
- Measure impact of JSON indexes (GIN, multi-valued)
- Compare indexed vs. non-indexed queries
- Target: 10-50x speedup with indexes

### Regression Tests

- Ensure existing JSON function support (JSON_EXTRACT, etc.) still works
- Verify no performance regression on non-JSON queries
- Test interaction with other optimization rules (join ordering, etc.)

## Performance Benchmarks

### Benchmark Queries

**B1: Simple Array Unnesting**
```sql
SELECT jt.item_id, jt.quantity
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id',
       quantity INT PATH '$.qty'
     )) AS jt;
```

**B2: Filtered Array Elements**
```sql
SELECT jt.item_id, jt.quantity
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id',
       quantity INT PATH '$.qty',
       price DECIMAL(10,2) PATH '$.price'
     )) AS jt
WHERE jt.price &gt; 100;
```

**B3: Nested JSON Structures**
```sql
SELECT jt.order_id, jt.item_id, jt.spec_name, jt.spec_value
FROM orders o,
     JSON_TABLE(o.order_data, '$' COLUMNS(
       order_id INT PATH '$.id',
       NESTED PATH '$.items[*]' COLUMNS(
         item_id INT PATH '$.id',
         NESTED PATH '$.specs[*]' COLUMNS(
           spec_name VARCHAR(50) PATH '$.name',
           spec_value VARCHAR(100) PATH '$.value'
         )
       )
     )) AS jt;
```

**B4: Large JSON Arrays**
```sql
-- 10,000 element array per row
SELECT jt.event_id, jt.timestamp, jt.event_type
FROM logs l,
     JSON_TABLE(l.events, '$[*]' COLUMNS(
       event_id INT PATH '$.id',
       timestamp TIMESTAMP PATH '$.ts',
       event_type VARCHAR(50) PATH '$.type'
     )) AS jt;
```

**B5: JSON with Index (PostgreSQL GIN)**
```sql
-- Assumes GIN index on orders.items
SELECT jt.item_id
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(
       item_id INT PATH '$.id'
     )) AS jt
WHERE jt.item_id = 12345;
```

### Expected Results

| Benchmark | Baseline (ms) | Optimized (ms) | Speedup | Optimization Applied |
|-----------|--------------|---------------|---------|---------------------|
| B1 (100 rows, 10 items each) | 150 | 45 | 3.3x | Column pruning, parallel unnest |
| B2 (100 rows, 10 items each) | 180 | 25 | 7.2x | Predicate pushdown |
| B3 (100 rows, 5 items, 3 specs) | 450 | 120 | 3.8x | Single-pass nested extraction |
| B4 (10 rows, 10K items each) | 8000 | 600 | 13.3x | Parallel unnesting (16 workers) |
| B5 (1M rows, 10 items each) | 45000 | 800 | 56x | JSON index scan |

**Overall expected impact:** 3-10x for typical queries, up to 50x with indexes.

## Drawbacks

**1. Parser Complexity:** JSON_TABLE adds significant syntax complexity (~1500 lines of parser code).

**2. JSONPath Dialect Fragmentation:** Different databases have subtle JSONPath differences. Supporting full JSONPath 1.0 requires careful compatibility testing.

**3. Cost Model Uncertainty:** Estimating JSON array cardinality without statistics is challenging. May require sampling or heuristics for initial queries.

**4. Nested JSON Overhead:** Deeply nested JSON (4+ levels) can still be slow even with optimization. May need to recommend schema redesign for such cases.

**5. Memory Usage:** Parallel unnesting of very large arrays can consume significant memory. Need limits to prevent OOM.

## Rationale and Alternatives

### Why JSON_TABLE over existing functions?

**Alternative 1: Continue using database-specific functions**
- PostgreSQL: `jsonb_array_elements`, `jsonb_to_recordset`
- MySQL: `JSON_EXTRACT` with manual array indexing
- SQL Server: `OPENJSON`

**Downsides:**
- Non-standard syntax across databases
- Verbose and error-prone
- Difficult for optimizer to recognize patterns
- Inconsistent error handling

**JSON_TABLE advantages:**
- Standard SQL:2016 syntax
- Declarative, optimizer-friendly
- Consistent semantics across databases

**Alternative 2: Virtual generated columns**

Create generated columns for frequently accessed JSON paths:

```sql
ALTER TABLE orders ADD COLUMN item_count AS (JSON_LENGTH(items)) STORED;
```

**Downsides:**
- Manual schema management
- Storage overhead for materialized columns
- Doesn't help with dynamic JSON queries
- Limited to simple extractions

**JSON_TABLE advantages:**
- No schema changes required
- Works with arbitrary JSON structures
- Query-time optimization

### Why not just support database-specific syntax?

**Option:** Support OPENJSON (SQL Server), FLATTEN (Snowflake), etc. directly.

**Downside:** Ra becomes a syntax translator, not an optimizer. No cross-database portability.

**JSON_TABLE approach:** Canonical representation with dialect translation enables cross-database query optimization.

## Prior Art

### Oracle JSON_TABLE (Oracle 12c+)

Oracle was first to implement SQL:2016 JSON_TABLE:

```sql
SELECT jt.*
FROM orders o,
     JSON_TABLE(o.order_data, '$.items[*]'
       COLUMNS(
         item_id NUMBER PATH '$.id',
         product VARCHAR2(100) PATH '$.name',
         quantity NUMBER PATH '$.qty'
       )
     ) jt;
```

**Features:**
- Full SQL:2016 compliance
- JSON search indexes for optimization
- Error handling with ON ERROR clause
- Nested arrays with NESTED PATH

**Optimizations:**
- Index-based path filtering
- Parallel query for large JSON arrays
- Statistics on JSON columns

### MySQL 8.0 JSON_TABLE

MySQL 8.0 added JSON_TABLE with binary JSON optimization:

```sql
SELECT jt.*
FROM orders o
JOIN JSON_TABLE(o.items, '$[*]'
  COLUMNS(
    item_id INT PATH '$.id',
    name VARCHAR(100) PATH '$.name'
  )
) AS jt;
```

**Features:**
- Binary JSON storage for fast access
- Multi-valued indexes on JSON arrays
- Generated column + index pattern

**Optimizations:**
- Multi-valued index usage for array elements
- Functional indexes on JSON_EXTRACT expressions

### PostgreSQL 17+ JSON_TABLE (Proposed)

PostgreSQL is implementing SQL:2016 JSON_TABLE:

```sql
SELECT *
FROM JSON_TABLE(
  '{"items": [{"id": 1}, {"id": 2}]}'::jsonb,
  '$.items[*]' COLUMNS(
    item_id int PATH '$.id'
  )
);
```

**Features:**
- JSONB binary format
- GIN indexes for path queries
- jsonb_path_query backend

**Optimizations:**
- GIN index scans for filtered queries
- Parallel workers for large arrays

### SQL Server OPENJSON

SQL Server uses OPENJSON instead of JSON_TABLE:

```sql
SELECT value
FROM OPENJSON(N'["a", "b", "c"]');

-- With schema
SELECT *
FROM OPENJSON(N'{"id": 1, "name": "test"}')
WITH (
  id INT '$.id',
  name VARCHAR(100) '$.name'
);
```

**Differences from JSON_TABLE:**
- No NESTED PATH (use multiple OPENJSON calls)
- No FOR ORDINALITY
- Different error handling semantics

**Optimizations:**
- Computed column indexes on JSON_VALUE
- Natively compiled operators

### Snowflake FLATTEN

Snowflake uses FLATTEN for JSON array unnesting:

```sql
SELECT f.value:id::INT AS item_id
FROM orders o,
     LATERAL FLATTEN(input =&gt; o.items) f;
```

**Differences:**
- No type specification in FLATTEN (uses :: casting)
- LATERAL join syntax instead of table function
- Works with VARIANT semi-structured type

**Optimizations:**
- Columnar storage for VARIANT fields
- Metadata-based pruning
- Clustering on VARIANT paths

## Unresolved Questions

1. **JSONPath Dialect Support:** Should Ra support full JSONPath 1.0, or subset to commonly-supported features?
   - **Recommendation:** Start with subset (`, ., [*], [n]), extend based on demand

2. **Statistics Collection:** How to gather JSON cardinality statistics without full table scan?
   - **Options:** Sampling, user hints, conservative defaults
   - **Recommendation:** Start with conservative defaults (assume 10 elements per array)

3. **Parallel Threshold:** At what array size should parallel unnesting activate?
   - **Recommendation:** 1000+ elements, configurable via optimizer parameter

4. **Index Detection:** How to reliably detect JSON-capable indexes across databases?
   - **Options:** Metadata queries, heuristics, manual hints
   - **Recommendation:** Database-specific metadata queries with fallback to table scan

5. **Cross-Database Semantics:** How to handle subtle differences in JSON_TABLE semantics?
   - **Example:** SQL Server OPENJSON returns NULL for missing paths, Oracle JSON_TABLE can return ERROR
   - **Recommendation:** Document differences, default to most permissive behavior (NULL ON ERROR)

## Future Possibilities

### JSON_TABLE Extensions

**1. PASSING clause for parameterized queries:**
```sql
JSON_TABLE(
  doc, '$.items[*]' PASSING @min_price AS min_price
  COLUMNS(
    item_id INT PATH '$.id',
    price DECIMAL PATH '$.price' WHERE @.price &gt; min_price
  )
)
```

**2. JSON Schema validation:**
```sql
JSON_TABLE(
  doc, '$' COLUMNS(...) CONFORMING TO json_schema_doc
)
```

**3. Streaming JSON processing:**
```sql
-- Process JSON as stream without full parse
JSON_TABLE(
  doc, '$[*]' COLUMNS(...) STREAMING
)
```

### JSON Optimization Framework

**1. Automatic JSON column statistics:**
- Gather array size distributions
- Track common paths and access patterns
- Recommend indexes based on query workload

**2. JSON-specific materialized views:**
- Pre-materialize frequently unnested JSON arrays
- Incremental maintenance on JSON updates

**3. JSON column recommendations:**
- Suggest converting JSON columns to relational tables when access patterns are regular
- Detect schema within "schemaless" JSON

### Cross-Feature Integration

**1. JSON_TABLE with window functions:**
```sql
SELECT
  jt.item_id,
  jt.quantity,
  SUM(jt.quantity) OVER (PARTITION BY jt.category ORDER BY jt.item_id)
FROM orders o,
     JSON_TABLE(o.items, '$[*]' COLUMNS(...)) jt;
```

**2. JSON_TABLE with temporal queries:**
```sql
-- Query historical JSON structure
SELECT * FROM orders FOR SYSTEM_TIME AS OF '2024-01-01'
CROSS JOIN JSON_TABLE(orders.items, '$[*]' COLUMNS(...));
```

## References

### SQL Standards
- **ISO/IEC 9075-2:2016** - SQL/Foundation (JSON_TABLE specification)
- **JSONPath Specification:** RFC 9535 (2024) - https://www.rfc-editor.org/rfc/rfc9535.html

### Database Documentation
- **Oracle JSON_TABLE:** https://docs.oracle.com/en/database/oracle/oracle-database/21/sqlrf/JSON_TABLE.html
- **MySQL JSON_TABLE:** https://dev.mysql.com/doc/refman/8.0/en/json-table-functions.html
- **PostgreSQL JSON Functions:** https://www.postgresql.org/docs/current/functions-json.html
- **SQL Server OPENJSON:** https://learn.microsoft.com/en-us/sql/t-sql/functions/openjson-transact-sql
- **Snowflake FLATTEN:** https://docs.snowflake.com/en/sql-reference/functions/flatten

### Research Papers
- **"Querying JSON: A Survey"** - Benedikt et al. (2023)
- **"Efficient Query Processing on Unstructured Tetrahedral Meshes"** - JSONPath optimization techniques
- **"The Ubiquity of Large Graphs and Surprising Challenges of Graph Processing"** - Applicable to JSON graph structures

### Related RFCs
- **[RFC 0083](/maintainers/rfcs/0083-xpath-xquery-optimization):** XPath and XQuery Optimization (similar structural query optimization)
- **[RFC 0084](/maintainers/rfcs/0084-oracle-json-relational-duality-optimization):** Oracle JSON Relational Duality View Optimization
- **[RFC 0093](/maintainers/rfcs/0093-sql-property-graph-queries):** SQL Property Graph Queries (graph pattern matching)

---

## Summary

JSON_TABLE is a critical feature for modern SQL workloads, with support across all 8 major databases. Implementing JSON_TABLE in Ra will:

1. Enable 3-10x optimization for JSON queries (up to 50x with indexes)
2. Provide standard SQL:2016 syntax across databases
3. Unlock advanced optimizations (predicate pushdown, parallel unnesting, index usage)
4. Position Ra as a leader in JSON query optimization

**Estimated effort:** 17-21 weeks (4-5 months)
**Expected impact:** High - JSON is ubiquitous in modern applications
**Risk:** Medium - Parser complexity and cost model uncertainty
**Priority:** High - #1 missing standard SQL feature by impact


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 94: JSON_TABLE Optimization](/maintainers/rfcs/0094-json-table-optimization)
