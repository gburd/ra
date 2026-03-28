# MySQL/MariaDB Unsupported Features Analysis

**Date**: 2026-03-28
**Purpose**: Comprehensive analysis of MySQL/MariaDB-specific features not currently supported by Ra optimizer
**Scope**: MySQL 5.7+, MySQL 8.0+, MariaDB 10.3+

---

## Executive Summary

This report catalogs MySQL/MariaDB-specific features that are not currently supported by the Ra query optimizer. While Ra has 26 MySQL-specific optimization rules covering core operations (joins, indexes, partitioning, window functions), several advanced MySQL/MariaDB extensions remain unimplemented.

**Coverage Status**:
- ✅ **Supported**: Basic SQL, joins, indexes, partitioning, window functions, CTEs
- ⚠️ **Partial**: Spatial data (generic GIS rules exist, MySQL-specific optimizations missing)
- ❌ **Unsupported**: Full-text search, JSON functions, storage engine hints, temporal tables, sequences

---

## 1. Full-Text Search (MATCH...AGAINST)

### Feature Description

MySQL and MariaDB provide full-text search capabilities for natural language queries on `TEXT` and `VARCHAR` columns using `FULLTEXT` indexes.

**Syntax**:
```sql
SELECT * FROM articles
WHERE MATCH(title, body) AGAINST ('database optimization' IN NATURAL LANGUAGE MODE);

SELECT * FROM articles
WHERE MATCH(title) AGAINST ('+mysql -postgresql' IN BOOLEAN MODE);

SELECT * FROM articles
WHERE MATCH(content) AGAINST ('query' WITH QUERY EXPANSION);
```

### MySQL vs MariaDB Differences

| Feature | MySQL 5.7+ | MySQL 8.0+ | MariaDB 10.3+ |
|---------|-----------|-----------|---------------|
| **InnoDB Full-Text** | ✅ | ✅ | ✅ |
| **MyISAM Full-Text** | ✅ | ✅ | ✅ |
| **Boolean Mode** | ✅ | ✅ | ✅ |
| **Natural Language** | ✅ | ✅ | ✅ |
| **Query Expansion** | ✅ | ✅ | ✅ |
| **NGRAM Parser** | ✅ (5.7.6+) | ✅ | ✅ (10.2+) |
| **MeCab Parser** | ❌ | ✅ (Japanese) | ❌ |
| **Stopwords Custom** | ✅ | ✅ | ✅ |
| **ft_min_word_length** | Default: 4 | Default: 4 | Default: 4 |

**Boolean Mode Operators**:
- `+` word must be present
- `-` word must not be present
- `>` increase word's relevance
- `<` decrease word's relevance
- `()` grouping
- `~` negation (treat as negative contribution)
- `*` wildcard suffix
- `"phrase"` exact phrase matching

### Use Cases

1. **Content Search**: Blog posts, documentation, article archives
2. **Product Catalogs**: E-commerce product descriptions and reviews
3. **Log Analysis**: Searching application logs for error patterns
4. **Knowledge Bases**: FAQ systems, support ticket search

### Implementation Complexity

**Difficulty**: 🟡 **Medium-High**

**Requirements**:
1. Parse `MATCH...AGAINST` syntax in SQL parser
2. Represent full-text predicates in `RelExpr` (new `Expr` variant)
3. Model `FULLTEXT` indexes in catalog metadata
4. Cost model: relevance scoring vs table scan
5. Handle stop words, minimum word length configuration
6. Support three search modes (natural language, boolean, query expansion)

**Estimated Effort**: 3-4 weeks

**Dependencies**:
- Extend `ra-core::Expr` with `FullTextMatch` variant
- Extend `ra-metadata` to detect `FULLTEXT` indexes
- Add full-text specific cost model (relevance ranking, index selectivity)

### Optimization Opportunities

#### Rule 1: Full-Text Index Selection
```
σ[MATCH(col) AGAINST(text)](scan(T))
  → fulltext_index_scan(T.ft_idx, text)
```
**Benefit**: 50-99% cost reduction vs table scan for text-heavy tables

#### Rule 2: Full-Text + Filter Pushdown
```
σ[predicate AND MATCH(...)](scan(T))
  → σ[predicate](fulltext_index_scan(T.ft_idx, text))
```
**Benefit**: Apply additional filters after full-text scan, reducing result set early

#### Rule 3: Relevance-Based Ordering
```
sort[relevance_score](σ[MATCH(...)](scan(T)))
  → fulltext_index_scan_ordered(T.ft_idx, text)
```
**Benefit**: MySQL full-text indexes return results in relevance order; avoid separate sort

#### Rule 4: Boolean Mode Short-Circuit
```
MATCH(...) AGAINST('+required -excluded +term1 +term2')
  → intersection(ft_scan(required), complement(ft_scan(excluded)), ...)
```
**Benefit**: Process boolean operators as set operations on posting lists

---

## 2. JSON Functions

### Feature Description

MySQL 8.0 and MariaDB 10.2+ provide extensive JSON manipulation and querying functions. JSON columns use binary storage format for efficient access.

### Core JSON Functions

#### Creation and Construction

| Function | MySQL 8.0 | MariaDB 10.2+ | Description |
|----------|-----------|---------------|-------------|
| `JSON_ARRAY(...)` | ✅ | ✅ | Create JSON array |
| `JSON_OBJECT(key, val, ...)` | ✅ | ✅ | Create JSON object |
| `JSON_QUOTE(str)` | ✅ | ✅ | Quote string as JSON value |

#### Extraction and Querying

| Function | MySQL 8.0 | MariaDB 10.2+ | Description |
|----------|-----------|---------------|-------------|
| `JSON_EXTRACT(doc, path)` | ✅ | ✅ | Extract value via JSONPath |
| `->` operator | ✅ | ✅ | Shorthand for JSON_EXTRACT (unquoted) |
| `->>` operator | ✅ | ✅ | Shorthand for JSON_UNQUOTE(JSON_EXTRACT(...)) |
| `JSON_VALUE(doc, path)` | ❌ | ✅ (10.2.3+) | Extract scalar value |
| `JSON_QUERY(doc, path)` | ❌ | ✅ (10.2.3+) | Extract object/array |
| `JSON_TABLE(doc, path COLUMNS(...))` | ✅ (8.0+) | ✅ (10.6+) | Convert JSON to relational table |
| `JSON_CONTAINS(doc, val, path)` | ✅ | ✅ | Check if value/subobject exists |
| `JSON_CONTAINS_PATH(doc, mode, path, ...)` | ✅ | ✅ | Check if path(s) exist |
| `JSON_SEARCH(doc, mode, str, esc, path)` | ✅ | ✅ | Find path to value |
| `JSON_KEYS(doc, path)` | ✅ | ✅ | Get object keys as array |

#### Modification

| Function | MySQL 8.0 | MariaDB 10.2+ | Description |
|----------|-----------|---------------|-------------|
| `JSON_SET(doc, path, val, ...)` | ✅ | ✅ | Insert or update values |
| `JSON_INSERT(doc, path, val, ...)` | ✅ | ✅ | Insert without replacing existing |
| `JSON_REPLACE(doc, path, val, ...)` | ✅ | ✅ | Replace existing values only |
| `JSON_REMOVE(doc, path, ...)` | ✅ | ✅ | Remove values at paths |
| `JSON_ARRAY_APPEND(doc, path, val, ...)` | ✅ | ✅ | Append to array |
| `JSON_ARRAY_INSERT(doc, path, val, ...)` | ✅ | ✅ | Insert into array at position |
| `JSON_MERGE_PATCH(doc1, doc2)` | ✅ (8.0+) | ✅ (10.2.3+) | RFC 7396 merge (key overwriting) |
| `JSON_MERGE_PRESERVE(doc1, doc2)` | ✅ (8.0+) | ✅ | Array/object merging |

#### Analysis and Validation

| Function | MySQL 8.0 | MariaDB 10.2+ | Description |
|----------|-----------|---------------|-------------|
| `JSON_TYPE(val)` | ✅ | ✅ | Return JSON value type |
| `JSON_VALID(val)` | ✅ | ✅ | Check if string is valid JSON |
| `JSON_LENGTH(doc, path)` | ✅ | ✅ | Count elements/properties |
| `JSON_DEPTH(doc)` | ✅ | ✅ | Maximum nesting depth |
| `JSON_SCHEMA_VALID(schema, doc)` | ❌ | ✅ (11.1+) | JSON Schema validation |

#### Formatting

| Function | MySQL 8.0 | MariaDB 10.2+ | Description |
|----------|-----------|---------------|-------------|
| `JSON_PRETTY(doc)` | ✅ | ✅ | Format with indentation |
| `JSON_STORAGE_SIZE(doc)` | ✅ | ❌ | Binary storage size |
| `JSON_STORAGE_FREE(doc)` | ✅ | ❌ | Free space after partial update |

### MySQL vs MariaDB Differences

**MySQL 8.0 Advantages**:
1. **Multi-Valued Indexes**: Index on JSON arrays
   ```sql
   CREATE INDEX idx ON t((CAST(data->'$.tags' AS UNSIGNED ARRAY)));
   ```
2. **Partial Updates**: In-place binary JSON updates without full rewrite
3. **JSON_SCHEMA_VALID** (8.0.17+): JSON Schema draft-07 validation

**MariaDB Advantages**:
1. **JSON_VALUE** and **JSON_QUERY**: SQL standard syntax (SQL:2016)
2. **JSON_NORMALIZE**: Sort keys recursively for comparison
3. **JSON_EQUALS**: Direct equality comparison without string conversion
4. **JSON_OVERLAPS**: Check for shared elements

### Use Cases

1. **Schema Flexibility**: Store semi-structured data (user preferences, product attributes)
2. **API Integration**: Store and query JSON API responses
3. **Event Logging**: Store structured event data with varying fields
4. **Document Store**: Use MySQL as a document database alongside relational data

### Implementation Complexity

**Difficulty**: 🔴 **High**

**Requirements**:
1. Parse JSON operators (`->`, `->>`) and functions
2. Extend `Expr` with JSON path expressions
3. Model JSON column types and indexes in metadata
4. Cost model for JSON operations (path traversal, document size)
5. Handle JSON binary format storage (MySQL) vs text format (PostgreSQL-style)
6. Support multi-valued indexes (MySQL 8.0)
7. Implement `JSON_TABLE` as table-valued function

**Estimated Effort**: 6-8 weeks

**Dependencies**:
- JSON parsing and path evaluation library
- Extend `ra-core::Expr` with JSON-specific variants
- Catalog metadata for JSON column statistics (document size distribution, path cardinality)

### Optimization Opportunities

#### Rule 1: JSON Index Selection
```
σ[JSON_EXTRACT(doc, '$.field') = value](scan(T))
  → index_scan(T.json_idx, '$.field', value)
```
**Benefit**: 80-95% cost reduction when JSON column has functional index

#### Rule 2: JSON Path Pushdown
```
π[JSON_EXTRACT(doc, '$.a'), JSON_EXTRACT(doc, '$.b')](scan(T))
  → π[extract_paths(doc, ['$.a', '$.b'])](scan(T))
```
**Benefit**: Single binary JSON traversal for multiple paths

#### Rule 3: JSON Predicate to Expression Index
```
σ[JSON_EXTRACT(doc, '$.status') = 'active'](scan(T))
  → index_scan(T.status_idx)  -- functional index on expression
```
**Benefit**: Leverage MySQL's generated column + index pattern

#### Rule 4: JSON_TABLE Unnesting
```
lateral_join(T, JSON_TABLE(T.doc, '$[*]' COLUMNS(...)))
  → unnest_json_array(T.doc)
```
**Benefit**: Recognize `JSON_TABLE` as array unnesting; apply unnesting optimizations

---

## 3. Spatial Data Types and Functions (GIS Extensions)

### Feature Description

MySQL 5.7+ and MariaDB 10+ support spatial data types and geographic information system (GIS) functions based on OpenGIS standards.

### Spatial Data Types

| Type | Description | MySQL | MariaDB |
|------|-------------|-------|---------|
| `GEOMETRY` | Abstract base type | ✅ | ✅ |
| `POINT` | Single location (x, y) | ✅ | ✅ |
| `LINESTRING` | Series of points forming a line | ✅ | ✅ |
| `POLYGON` | Closed area with holes | ✅ | ✅ |
| `MULTIPOINT` | Collection of points | ✅ | ✅ |
| `MULTILINESTRING` | Collection of linestrings | ✅ | ✅ |
| `MULTIPOLYGON` | Collection of polygons | ✅ | ✅ |
| `GEOMETRYCOLLECTION` | Heterogeneous geometry collection | ✅ | ✅ |

### Spatial Functions

#### Geometry Construction

| Function | MySQL | MariaDB | Description |
|----------|-------|---------|-------------|
| `ST_GeomFromText(wkt, srid)` | ✅ | ✅ | Create from WKT |
| `ST_GeomFromWKB(wkb, srid)` | ✅ | ✅ | Create from WKB |
| `ST_Point(x, y)` | ✅ | ✅ | Create point |
| `ST_LineFromText(wkt, srid)` | ✅ | ✅ | Create linestring |
| `ST_PolygonFromText(wkt, srid)` | ✅ | ✅ | Create polygon |

#### Spatial Relationships (Predicates)

| Function | MySQL 5.7 | MySQL 8.0 | MariaDB | Description |
|----------|-----------|-----------|---------|-------------|
| `ST_Contains(g1, g2)` | ✅ | ✅ | ✅ | g1 contains g2 |
| `ST_Within(g1, g2)` | ✅ | ✅ | ✅ | g1 is within g2 |
| `ST_Intersects(g1, g2)` | ✅ | ✅ | ✅ | Geometries intersect |
| `ST_Crosses(g1, g2)` | ✅ | ✅ | ✅ | Geometries cross |
| `ST_Touches(g1, g2)` | ✅ | ✅ | ✅ | Geometries touch |
| `ST_Overlaps(g1, g2)` | ✅ | ✅ | ✅ | Geometries overlap |
| `ST_Disjoint(g1, g2)` | ✅ | ✅ | ✅ | Geometries are disjoint |
| `ST_Equals(g1, g2)` | ✅ | ✅ | ✅ | Geometries are equal |
| `MBRContains(g1, g2)` | ✅ | ✅ | ✅ | MBR (bounding box) contains |
| `MBRIntersects(g1, g2)` | ✅ | ✅ | ✅ | MBR intersects (faster) |

#### Spatial Analysis

| Function | MySQL 5.7 | MySQL 8.0 | MariaDB | Description |
|----------|-----------|-----------|---------|-------------|
| `ST_Distance(g1, g2)` | ✅ | ✅ | ✅ | Distance between geometries |
| `ST_Distance_Sphere(g1, g2)` | ✅ | ✅ | ✅ | Great-circle distance (Earth) |
| `ST_Area(polygon)` | ✅ | ✅ | ✅ | Area of polygon |
| `ST_Length(linestring)` | ✅ | ✅ | ✅ | Length of linestring |
| `ST_Buffer(geom, dist)` | ❌ | ✅ (8.0.2+) | ✅ | Buffer zone around geometry |
| `ST_ConvexHull(geom)` | ❌ | ✅ (8.0.2+) | ✅ | Convex hull |
| `ST_Union(g1, g2)` | ❌ | ✅ (8.0.2+) | ✅ | Union of geometries |
| `ST_Intersection(g1, g2)` | ❌ | ✅ (8.0.2+) | ✅ | Intersection of geometries |
| `ST_Difference(g1, g2)` | ❌ | ✅ (8.0.2+) | ✅ | Difference of geometries |

### Spatial Indexes

**Index Type**: `SPATIAL` (R-tree based)

```sql
CREATE SPATIAL INDEX idx_location ON places(geom);

SELECT * FROM places
WHERE ST_Contains(
  ST_GeomFromText('POLYGON((...))', 4326),
  geom
);
```

### MySQL vs MariaDB Differences

**MySQL 8.0 Enhancements**:
1. GIS functions use `ST_` prefix (OpenGIS standard)
2. Support for geographic (spherical) SRS transformations
3. Spatial join optimization in optimizer
4. More complete set of analysis functions (ST_Buffer, ST_Union, etc.)

**MariaDB**:
1. Largely compatible with MySQL spatial functions
2. Some functions use `MBR` prefix for bounding-box-only operations (faster)

### Use Cases

1. **Location Services**: Find nearby places, geofencing
2. **Logistics**: Route planning, delivery zones
3. **Real Estate**: Property searches within boundaries
4. **GIS Applications**: Mapping, spatial analysis
5. **IoT**: Device tracking, sensor coverage areas

### Implementation Complexity

**Difficulty**: 🟡 **Medium**

**Status**: Ra has generic spatial optimization rules (see `/rules/logical/function-optimization/geospatial-function-optimization.rra`), but lacks MySQL-specific optimizations.

**Missing MySQL-Specific Optimizations**:
1. **MBR Bounding Box Pre-Filter**: MySQL optimizer adds `MBRIntersects` before expensive `ST_Intersects`
2. **Spatial Index Selection**: Detect `SPATIAL` indexes on geometry columns
3. **Spatial Join Strategies**: MySQL 8.0 has specialized spatial join algorithms
4. **Distance-Based Queries**: `ST_Distance` optimization with spatial indexes

**Estimated Effort**: 2-3 weeks (building on existing spatial rules)

### Optimization Opportunities

#### Rule 1: MBR Pre-Filter Injection
```
σ[ST_Contains(g1, g2)](scan(T))
  → σ[ST_Contains(g1, g2)](σ[MBRContains(g1, g2)](scan(T)))
```
**Benefit**: MBR check is O(1) bounding box comparison; filters 80-95% of candidates

#### Rule 2: Spatial Index Selection
```
σ[ST_Within(location, region)](scan(places))
  → spatial_index_scan(places.spatial_idx, region)
```
**Benefit**: R-tree spatial index reduces search from O(n) to O(log n)

#### Rule 3: Distance Query to Range Query
```
σ[ST_Distance(p1, p2) < radius](scan(T))
  → spatial_index_scan(T.spatial_idx, ST_Buffer(p2, radius))
```
**Benefit**: Convert distance check to range query on spatial index

---

## 4. Window Functions (MariaDB Extensions)

### Feature Description

MySQL 8.0 introduced window functions. MariaDB 10.2+ also supports window functions with some additional extensions.

### Ra Support Status

✅ **Supported**: Ra has MySQL window function optimization rules (see `/rules/database-specific/mysql/window-function-optimization.rra`)

### MariaDB-Specific Extensions

| Feature | MySQL 8.0 | MariaDB 10.2+ | Description |
|---------|-----------|---------------|-------------|
| **MEDIAN()** | ❌ | ✅ | Median aggregate function |
| **PERCENTILE_CONT()** | ❌ | ✅ | Continuous percentile |
| **PERCENTILE_DISC()** | ❌ | ✅ | Discrete percentile |

**Example**:
```sql
-- MariaDB-specific
SELECT dept, salary,
  MEDIAN(salary) OVER (PARTITION BY dept) AS median_sal,
  PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY salary) AS p95
FROM employees;
```

### Implementation Complexity

**Difficulty**: 🟢 **Low**

**Requirements**:
- Extend window function list with MariaDB-specific aggregates
- Add cost model for percentile calculations (sorting-based vs approximation)

**Estimated Effort**: 1 week

---

## 5. Common Table Expressions (CTEs)

### Feature Description

Both MySQL 8.0+ and MariaDB 10.2+ support CTEs and recursive CTEs.

### Ra Support Status

✅ **Supported**: Ra's SQL parser handles CTEs and recursive CTEs (see `/home/gburd/ws/ra/crates/ra-parser/src/sql_to_relexpr.rs` lines 1-14)

### MySQL vs MariaDB Differences

**No significant differences**: Both implement SQL:1999 standard `WITH` clause and `WITH RECURSIVE`.

**Example**:
```sql
WITH RECURSIVE cte AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM cte WHERE n < 10
)
SELECT * FROM cte;
```

### Optimization Opportunities

Ra already has CTE optimization rules:
- `/rules/logical/cte-optimization/recursive-cte-join-reorder.rra`
- `/rules/logical/cte-optimization/recursive-cte-to-iterative.rra`

---

## 6. Table Value Constructors (VALUES)

### Feature Description

Construct a temporary table from a list of row values.

**Syntax**:
```sql
-- MySQL 8.0.19+, MariaDB 10.3.3+
SELECT * FROM (VALUES ROW(1, 'a'), ROW(2, 'b'), ROW(3, 'c')) AS t(id, name);

-- Can be used in INSERT
INSERT INTO t VALUES (1, 'a'), (2, 'b'), (3, 'c');
```

### MySQL vs MariaDB Differences

| Feature | MySQL 8.0.19+ | MariaDB 10.3.3+ |
|---------|---------------|-----------------|
| **VALUES clause** | ✅ | ✅ |
| **In subqueries** | ✅ | ✅ |
| **Without ROW()** | ✅ (8.0.19+) | ✅ |

### Ra Support Status

❌ **Unsupported**: SQL parser likely handles `VALUES` in `INSERT`, but not as a standalone table constructor in `FROM`.

### Implementation Complexity

**Difficulty**: 🟢 **Low**

**Requirements**:
1. Parse `VALUES` as a table expression
2. Represent as `RelExpr::Values(Vec<Vec<Const>>)`
3. Cost model: O(num_rows) materialization cost

**Estimated Effort**: 1 week

### Optimization Opportunities

#### Rule 1: VALUES Inlining
```
σ[predicate](VALUES (...))
  → VALUES (filtered_rows)
```
**Benefit**: Constant folding eliminates rows at plan time

#### Rule 2: VALUES Join Reordering
```
JOIN(T1, VALUES(...), T3)
  → JOIN(T1, T3, VALUES(...))  -- if VALUES is small
```
**Benefit**: Join smallest table first

---

## 7. INTERSECT/EXCEPT Set Operations

### Feature Description

Set operations beyond `UNION`.

**Syntax**:
```sql
SELECT col FROM t1
INTERSECT
SELECT col FROM t2;

SELECT col FROM t1
EXCEPT
SELECT col FROM t2;
```

### MySQL vs MariaDB Differences

| Feature | MySQL 5.7 | MySQL 8.0 | MariaDB 10.3+ |
|---------|-----------|-----------|---------------|
| **INTERSECT** | ❌ | ❌ | ✅ (10.3+) |
| **EXCEPT** | ❌ | ❌ | ✅ (10.3+) |
| **INTERSECT ALL** | ❌ | ❌ | ✅ |
| **EXCEPT ALL** | ❌ | ❌ | ✅ |

**Note**: MySQL 8.0 still does not support `INTERSECT`/`EXCEPT` natively. They must be emulated with joins:

```sql
-- Emulate INTERSECT
SELECT DISTINCT t1.col FROM t1 INNER JOIN t2 ON t1.col = t2.col;

-- Emulate EXCEPT
SELECT t1.col FROM t1 LEFT JOIN t2 ON t1.col = t2.col WHERE t2.col IS NULL;
```

### Ra Support Status

✅ **Partially Supported**: SQL parser handles `INTERSECT`/`EXCEPT` (see `/home/gburd/ws/ra/crates/ra-dialect/src/dialect.rs` line 337-344)

Dialect feature check shows `supports_intersect()` returns `true` for all dialects, but this is incorrect for MySQL.

### Implementation Complexity

**Difficulty**: 🟢 **Low**

**Requirements**:
1. Correct dialect feature detection (MySQL does NOT support `INTERSECT`/`EXCEPT`)
2. Emulation rules to rewrite as joins when translating to MySQL

**Estimated Effort**: 3-5 days

### Optimization Opportunities

#### Rule 1: INTERSECT to Semi-Join
```
INTERSECT(T1, T2)
  → semi_join(T1, T2, T1.* = T2.*)
```
**Benefit**: Avoid materializing second result set

#### Rule 2: EXCEPT to Anti-Join
```
EXCEPT(T1, T2)
  → anti_join(T1, T2, T1.* = T2.*)
```
**Benefit**: More efficient than `LEFT JOIN ... WHERE NULL`

---

## 8. Sequences (MariaDB Only)

### Feature Description

MariaDB 10.3+ provides sequence generators for generating series of numbers.

**Syntax**:
```sql
-- Create a sequence
CREATE SEQUENCE seq_id START WITH 1 INCREMENT BY 1;

-- Use in queries
SELECT NEXT VALUE FOR seq_id;

SELECT PREVIOUS VALUE FOR seq_id;

-- Generate series
SELECT * FROM seq_1_to_100;  -- Built-in sequence table
```

### MySQL vs MariaDB Differences

| Feature | MySQL 8.0 | MariaDB 10.3+ |
|---------|-----------|---------------|
| **CREATE SEQUENCE** | ❌ | ✅ |
| **NEXT VALUE FOR** | ❌ | ✅ |
| **Sequence Tables** | ❌ | ✅ |

**MySQL Alternative**: Use `AUTO_INCREMENT` columns or generate series with recursive CTEs.

### Ra Support Status

❌ **Unsupported**: No sequence support in SQL parser or metadata layer.

### Use Cases

1. **ID Generation**: Generate unique IDs outside table context
2. **Series Generation**: `SELECT * FROM seq_1_to_1000` for calendar tables
3. **Gap Analysis**: Find missing IDs in sequence

### Implementation Complexity

**Difficulty**: 🟡 **Medium**

**Requirements**:
1. Parse `CREATE SEQUENCE`, `NEXT VALUE FOR`, `PREVIOUS VALUE FOR`
2. Model sequences in catalog metadata
3. Represent sequence tables (`seq_N_to_M`) as table functions
4. Cost model: O(1) for sequence access, O(n) for sequence table generation

**Estimated Effort**: 2-3 weeks

### Optimization Opportunities

#### Rule 1: Sequence Table to Generate Series
```
scan(seq_1_to_1000)
  → generate_series(1, 1000, 1)
```
**Benefit**: Recognize as table-valued function; apply pushdown rules

#### Rule 2: Sequence Allocation Batching
```
NEXT VALUE FOR seq (called N times in a batch)
  → allocate_batch(seq, N)
```
**Benefit**: Reduce lock contention by pre-allocating sequence values

---

## 9. Temporal Tables (MariaDB System-Versioned Tables)

### Feature Description

MariaDB 10.3+ supports system-versioned tables for automatic row history tracking.

**Syntax**:
```sql
CREATE TABLE employees (
  id INT PRIMARY KEY,
  name VARCHAR(100),
  salary DECIMAL(10,2)
) WITH SYSTEM VERSIONING;

-- Query current data (default)
SELECT * FROM employees;

-- Query historical data
SELECT * FROM employees FOR SYSTEM_TIME AS OF '2024-01-01 00:00:00';

-- Query all versions
SELECT * FROM employees FOR SYSTEM_TIME ALL;

-- Query range
SELECT * FROM employees FOR SYSTEM_TIME BETWEEN '2024-01-01' AND '2024-12-31';
```

### MySQL vs MariaDB Differences

| Feature | MySQL 8.0 | MariaDB 10.3+ |
|---------|-----------|---------------|
| **System Versioning** | ❌ | ✅ |
| **Application-Time Periods** | ❌ | ✅ (10.4+) |
| **Automatic History Tables** | ❌ | ✅ |
| **Temporal Queries** | ❌ | ✅ |

**MySQL Alternative**: Manually implement history tables with triggers.

### Ra Support Status

❌ **Unsupported**: No temporal query support.

### Use Cases

1. **Audit Trails**: Track all changes to sensitive data
2. **Compliance**: GDPR, SOX, HIPAA historical data requirements
3. **Time Travel Debugging**: Investigate data state at specific points in time
4. **Rollback**: Restore previous data versions

### Implementation Complexity

**Difficulty**: 🔴 **High**

**Requirements**:
1. Parse `FOR SYSTEM_TIME` temporal query syntax
2. Model system-versioned tables in metadata (row_start, row_end columns)
3. Rewrite temporal queries to filter on hidden temporal columns
4. Cost model: account for scanning history table partitions
5. Handle temporal join semantics

**Estimated Effort**: 4-6 weeks

### Optimization Opportunities

#### Rule 1: Temporal Predicate Pushdown
```
σ[predicate](scan(T) FOR SYSTEM_TIME AS OF t)
  → σ[predicate AND row_start <= t AND row_end > t](scan(T_history))
```
**Benefit**: Push temporal filter down to storage layer; avoid scanning all history

#### Rule 2: Temporal Partition Pruning
```
scan(T FOR SYSTEM_TIME BETWEEN t1 AND t2)
  → scan(T_history[partitions_between(t1, t2)])
```
**Benefit**: Eliminate partitions outside temporal range

#### Rule 3: Temporal Join Alignment
```
JOIN(T1 FOR SYSTEM_TIME AS OF t, T2 FOR SYSTEM_TIME AS OF t)
  → temporal_aligned_join(T1, T2, t)
```
**Benefit**: Single temporal filter; ensure matching versions joined

---

## 10. Partitioning

### Feature Description

MySQL and MariaDB support table partitioning to divide large tables into smaller, manageable pieces.

### Ra Support Status

✅ **Supported**: Ra has MySQL partition pruning rule (see `/rules/database-specific/mysql/partition-pruning.rra`)

### Partitioning Types

| Type | MySQL 5.7+ | MySQL 8.0+ | MariaDB 10.3+ | Description |
|------|-----------|-----------|---------------|-------------|
| **RANGE** | ✅ | ✅ | ✅ | Partition by value ranges |
| **LIST** | ✅ | ✅ | ✅ | Partition by discrete value lists |
| **HASH** | ✅ | ✅ | ✅ | Partition by hash function |
| **KEY** | ✅ | ✅ | ✅ | Partition by hashed key columns |
| **Subpartitioning** | ✅ | ✅ | ✅ | RANGE/LIST with HASH/KEY subpartitions |

**Example**:
```sql
CREATE TABLE sales (
  id INT,
  sale_date DATE,
  amount DECIMAL(10,2)
)
PARTITION BY RANGE (YEAR(sale_date)) (
  PARTITION p2022 VALUES LESS THAN (2023),
  PARTITION p2023 VALUES LESS THAN (2024),
  PARTITION p2024 VALUES LESS THAN (2025)
);
```

### Missing Optimizations

While Ra has basic partition pruning, it lacks:

1. **Subpartition Pruning**: When both partition and subpartition keys are in predicates
2. **Partition-Wise Joins**: Join matching partitions directly
3. **Dynamic Partition Pruning**: Prune partitions based on runtime values from join

**Estimated Effort**: 2-3 weeks for advanced partition optimizations

---

## 11. Storage Engine Specific Features

### Feature Description

MySQL supports pluggable storage engines with different characteristics.

### Storage Engines

| Engine | MySQL 5.7 | MySQL 8.0 | MariaDB 10.3+ | Characteristics |
|--------|-----------|-----------|---------------|-----------------|
| **InnoDB** | ✅ (Default) | ✅ (Default) | ✅ (Default) | ACID, transactions, foreign keys, row-level locking |
| **MyISAM** | ✅ | ✅ | ✅ | Fast reads, table-level locking, no transactions |
| **Aria** | ❌ | ❌ | ✅ | MariaDB's crash-safe MyISAM replacement |
| **Memory (HEAP)** | ✅ | ✅ | ✅ | In-memory tables, fast but volatile |
| **CSV** | ✅ | ✅ | ✅ | Store data as CSV files |
| **Archive** | ✅ | ✅ | ✅ | Compressed, insert-only storage |
| **Federated** | ✅ | ❌ (removed) | ✅ | Access remote MySQL tables |

### Engine-Specific Features

#### InnoDB
- **Adaptive Hash Index**: Automatic in-memory hash index for hot pages
- **Buffer Pool**: Configurable memory cache for data and indexes
- **Change Buffer**: Cache for secondary index changes
- **Doublewrite Buffer**: Data integrity protection
- **Foreign Keys**: Referential integrity constraints

#### MyISAM
- **Compressed Tables**: `myisampack` for read-only compressed tables
- **Full-Text Search**: Native full-text indexes (before InnoDB added support)

#### Aria (MariaDB)
- **Crash-Safe**: Transactional DDL, crash recovery
- **Page Cache**: Separate from InnoDB buffer pool
- **Bulk Insert Optimization**: Fast LOAD DATA operations

### Ra Support Status

❌ **Unsupported**: No storage engine awareness in cost model or metadata.

### Use Cases

1. **Engine Selection**: Choose InnoDB for transactional tables, MyISAM for read-heavy analytics
2. **Memory Tables**: Use for session data, temporary results
3. **Archive Tables**: Long-term log storage with high compression

### Implementation Complexity

**Difficulty**: 🟡 **Medium**

**Requirements**:
1. Extend metadata layer to capture storage engine type
2. Engine-specific cost models:
   - MyISAM: table-level locking cost
   - InnoDB: transaction overhead, buffer pool hit rate
   - Memory: O(1) access, no persistence
3. Query hints for engine-specific optimizations (e.g., `USE INDEX`)

**Estimated Effort**: 3-4 weeks

### Optimization Opportunities

#### Rule 1: Memory Table for Temporary Results
```
materialize(intermediate_result)
  → create_memory_table(intermediate_result)  -- if small enough
```
**Benefit**: 10-100x faster for small intermediate results

#### Rule 2: MyISAM Table Lock Escalation
```
UPDATE myisam_table SET col = val WHERE predicate
  → LOCK TABLES myisam_table WRITE; UPDATE ...; UNLOCK TABLES;
```
**Benefit**: Explicit locking avoids lock contention for batch updates

#### Rule 3: InnoDB Buffer Pool Awareness
```
JOIN(large_table1, large_table2)
  → nested_loop_join(large_table1, large_table2)  -- if T2 fits in buffer pool
  → hash_join(large_table1, large_table2)         -- otherwise
```
**Benefit**: Cost model accounts for buffer pool size; avoid disk I/O when possible

---

## 12. Index Hints and Optimizer Hints

### Feature Description

MySQL and MariaDB allow explicit control over index selection and join order via hints.

### Index Hints

**Syntax**:
```sql
SELECT * FROM t1 USE INDEX (idx_name) WHERE col = 10;
SELECT * FROM t1 IGNORE INDEX (idx_name) WHERE col = 10;
SELECT * FROM t1 FORCE INDEX (idx_name) WHERE col = 10;
```

### Optimizer Hints (MySQL 8.0+)

**Syntax**:
```sql
SELECT /*+ BKA(t1) */ * FROM t1 JOIN t2 ON t1.id = t2.id;
SELECT /*+ HASH_JOIN(t1, t2) */ * FROM t1 JOIN t2 ON t1.id = t2.id;
SELECT /*+ NO_INDEX(t1 idx_name) */ * FROM t1 WHERE col = 10;
SELECT /*+ MAX_EXECUTION_TIME(1000) */ * FROM t1 WHERE col > 100;
```

### MySQL vs MariaDB Differences

| Feature | MySQL 5.7 | MySQL 8.0 | MariaDB 10.3+ |
|---------|-----------|-----------|---------------|
| **Index Hints** | ✅ | ✅ | ✅ |
| **Optimizer Hints** | Limited | ✅ (50+ hints) | Limited |
| **Join Order Hints** | ❌ | ✅ (`JOIN_ORDER`) | ❌ |
| **Subquery Hints** | ❌ | ✅ (`SEMIJOIN`, `NO_SEMIJOIN`) | ❌ |

### Ra Support Status

❌ **Unsupported**: No hint parsing or hint-driven optimization.

### Use Cases

1. **Override Optimizer**: Force specific index when optimizer chooses poorly
2. **Performance Tuning**: Pin query plans in production
3. **Testing**: Isolate specific execution strategies

### Implementation Complexity

**Difficulty**: 🟡 **Medium**

**Requirements**:
1. Parse hint comments (`/*+ ... */` syntax)
2. Represent hints in `RelExpr` metadata
3. Apply hints as hard constraints in rule application
4. Validate hint compatibility (e.g., can't force non-existent index)

**Estimated Effort**: 2-3 weeks

### Optimization Opportunities

#### Rule 1: Hint-Driven Index Selection
```
σ[predicate](scan(T) USE INDEX (idx_name))
  → index_scan(T.idx_name, predicate)  -- ignore cost comparison
```
**Benefit**: Guarantee specific index used, bypassing optimizer decision

#### Rule 2: Join Order Enforcement
```
JOIN(T1, T2, T3) WITH HINT JOIN_ORDER(T3, T1, T2)
  → JOIN(JOIN(T3, T1), T2)  -- force specified order
```
**Benefit**: Pin join order for stable performance in production

---

## 13. Generated/Virtual Columns

### Feature Description

MySQL 5.7+ and MariaDB 10.2+ support generated columns that are computed from other columns.

**Syntax**:
```sql
CREATE TABLE users (
  id INT PRIMARY KEY,
  first_name VARCHAR(50),
  last_name VARCHAR(50),
  -- Virtual column (computed on read)
  full_name VARCHAR(101) AS (CONCAT(first_name, ' ', last_name)) VIRTUAL,
  -- Stored column (computed on write, stored on disk)
  full_name_upper VARCHAR(101) AS (UPPER(CONCAT(first_name, ' ', last_name))) STORED,
  INDEX idx_full_name (full_name_upper)
);

-- Query using generated column
SELECT * FROM users WHERE full_name_upper = 'JOHN DOE';
```

### MySQL vs MariaDB Differences

| Feature | MySQL 5.7+ | MySQL 8.0+ | MariaDB 10.2+ |
|---------|-----------|-----------|---------------|
| **VIRTUAL Columns** | ✅ | ✅ | ✅ |
| **STORED Columns** | ✅ | ✅ | ✅ (called PERSISTENT) |
| **Indexable** | STORED only | ✅ Both | STORED only |
| **JSON Path Indexes** | ❌ | ✅ (8.0.13+) | ❌ |

**MySQL 8.0 Enhancement**: Functional indexes on virtual columns
```sql
CREATE INDEX idx_json ON t ((JSON_EXTRACT(doc, '$.field')));
```

### Ra Support Status

⚠️ **Partial Support**: Some rules mention generated columns (see grep results), but no comprehensive support.

### Use Cases

1. **Denormalization**: Precompute expensive expressions
2. **Functional Indexes**: Index on computed values (e.g., JSON paths, expressions)
3. **Data Validation**: Computed check constraints
4. **Compatibility**: Expose computed columns for legacy applications

### Implementation Complexity

**Difficulty**: 🟡 **Medium**

**Requirements**:
1. Model generated columns in metadata (expression, virtual vs stored)
2. Recognize when predicates/projections can use generated column indexes
3. Cost model: VIRTUAL = expression eval cost, STORED = normal column access cost
4. Rewrite expressions to use generated columns when beneficial

**Estimated Effort**: 2-3 weeks

### Optimization Opportunities

#### Rule 1: Expression to Generated Column Index
```
σ[UPPER(name) = 'JOHN'](scan(T))
  → index_scan(T.name_upper_idx)  -- if name_upper is generated as UPPER(name)
```
**Benefit**: 80-95% cost reduction using index instead of computing expression for every row

#### Rule 2: Virtual Column Materialization
```
π[CONCAT(first_name, ' ', last_name), ...](scan(T)) WHERE ...
  → π[full_name, ...](scan(T)) WHERE ...  -- if full_name is virtual column
```
**Benefit**: Avoid recomputing expression; optimizer can recognize as single column access

#### Rule 3: JSON Function to Functional Index
```
σ[JSON_EXTRACT(doc, '$.status') = 'active'](scan(T))
  → index_scan(T.status_idx)  -- if status_idx is functional index on JSON path
```
**Benefit**: MySQL 8.0 functional indexes enable fast JSON queries

---

## 14. CHECK Constraints (MySQL 8.0+)

### Feature Description

MySQL 8.0.16+ supports `CHECK` constraints for column-level and table-level data validation.

**Syntax**:
```sql
CREATE TABLE products (
  id INT PRIMARY KEY,
  price DECIMAL(10,2) CHECK (price > 0),
  discount_pct INT CHECK (discount_pct BETWEEN 0 AND 100),
  CONSTRAINT chk_price_discount CHECK (discount_pct < 50 OR price > 100)
);
```

### MySQL vs MariaDB Differences

| Feature | MySQL 5.7 | MySQL 8.0.16+ | MariaDB 10.2+ |
|---------|-----------|---------------|---------------|
| **CHECK Constraints** | ❌ (parsed but ignored) | ✅ | ✅ |
| **Enforcement** | ❌ | ✅ | ✅ |
| **Alter Table** | ❌ | ✅ | ✅ |

### Ra Support Status

⚠️ **Partial Support**: Metadata layer queries constraints but may not use CHECK for optimization.

### Use Cases

1. **Data Validation**: Enforce business rules at database level
2. **Constraint-Based Optimization**: Eliminate impossible predicates
3. **Partition Pruning**: Use CHECK constraints to infer partition ranges

### Implementation Complexity

**Difficulty**: 🟢 **Low**

**Requirements**:
1. Parse CHECK constraint definitions from metadata
2. Use constraints for contradiction detection (e.g., `price > 0` AND query has `price < 0`)
3. Simplify predicates when constraints guarantee truth

**Estimated Effort**: 1-2 weeks

### Optimization Opportunities

#### Rule 1: Contradiction Detection via CHECK
```
σ[price < 0](scan(products))  -- products has CHECK (price > 0)
  → empty_result()
```
**Benefit**: Eliminate entire query at plan time

#### Rule 2: Redundant Predicate Elimination
```
σ[price > 0 AND price > 100](scan(products))  -- CHECK (price > 0)
  → σ[price > 100](scan(products))  -- price > 0 is redundant
```
**Benefit**: Simplify predicate evaluation

---

## 15. Invisible Indexes (MySQL 8.0+)

### Feature Description

MySQL 8.0+ allows marking indexes as "invisible" to the optimizer without dropping them.

**Syntax**:
```sql
CREATE INDEX idx_name ON t (col) INVISIBLE;
ALTER TABLE t ALTER INDEX idx_name INVISIBLE;
ALTER TABLE t ALTER INDEX idx_name VISIBLE;
```

### Ra Support Status

✅ **Partially Supported**: Ra has MySQL invisible index rule (see `/rules/database-specific/mysql/invisible-index.rra`)

### Use Cases

1. **Index Testing**: Test performance without an index before dropping
2. **Gradual Rollout**: Disable index on production, monitor performance
3. **Temporary Disable**: Skip expensive index maintenance during bulk loads

### Implementation Complexity

**Difficulty**: 🟢 **Low**

**Requirements**:
- Query `information_schema.STATISTICS.IS_VISIBLE` column
- Filter invisible indexes from available index list

**Estimated Effort**: Already implemented

---

## Summary Table: Feature Support Status

| Feature | Ra Support | Implementation Difficulty | Estimated Effort | Priority |
|---------|-----------|--------------------------|------------------|----------|
| **Full-Text Search** | ❌ | 🟡 Medium-High | 3-4 weeks | High |
| **JSON Functions** | ❌ | 🔴 High | 6-8 weeks | High |
| **Spatial/GIS** | ⚠️ Partial | 🟡 Medium | 2-3 weeks | Medium |
| **Window Functions** | ✅ Supported | - | - | - |
| **CTEs** | ✅ Supported | - | - | - |
| **Table Value Constructors** | ❌ | 🟢 Low | 1 week | Low |
| **INTERSECT/EXCEPT** | ⚠️ Partial | 🟢 Low | 3-5 days | Low |
| **Sequences** | ❌ | 🟡 Medium | 2-3 weeks | Low |
| **Temporal Tables** | ❌ | 🔴 High | 4-6 weeks | Medium |
| **Partitioning** | ✅ Supported | - | 2-3 weeks (advanced) | Medium |
| **Storage Engines** | ❌ | 🟡 Medium | 3-4 weeks | Medium |
| **Index/Optimizer Hints** | ❌ | 🟡 Medium | 2-3 weeks | Medium |
| **Generated Columns** | ⚠️ Partial | 🟡 Medium | 2-3 weeks | High |
| **CHECK Constraints** | ⚠️ Partial | 🟢 Low | 1-2 weeks | Low |
| **Invisible Indexes** | ✅ Supported | - | - | - |

---

## Recommended Implementation Priority

### Phase 1: High-Impact, Medium Complexity (6-8 weeks)
1. **JSON Functions** (6-8 weeks) - Critical for modern applications
2. **Full-Text Search** (3-4 weeks) - Common use case
3. **Generated Columns** (2-3 weeks) - Enables functional index optimizations

### Phase 2: Medium-Impact, Low-Medium Complexity (6-8 weeks)
4. **Spatial/GIS MySQL-Specific** (2-3 weeks) - Build on existing spatial rules
5. **Storage Engine Awareness** (3-4 weeks) - Cost model improvements
6. **Index/Optimizer Hints** (2-3 weeks) - Production tuning capability

### Phase 3: Specialized Features (8-10 weeks)
7. **Temporal Tables** (4-6 weeks) - MariaDB-specific, niche use case
8. **Advanced Partitioning** (2-3 weeks) - Partition-wise joins, dynamic pruning
9. **Sequences** (2-3 weeks) - MariaDB-only

### Phase 4: Low-Priority (2-3 weeks)
10. **CHECK Constraints** (1-2 weeks) - Contradiction detection
11. **Table Value Constructors** (1 week) - Nice-to-have
12. **INTERSECT/EXCEPT Emulation** (3-5 days) - Fix dialect detection

---

## References

### MySQL Documentation
- MySQL 8.0 Reference Manual: https://dev.mysql.com/doc/refman/8.0/en/
- MySQL 5.7 Reference Manual: https://dev.mysql.com/doc/refman/5.7/en/
- MySQL Optimizer Internals: https://dev.mysql.com/doc/internals/en/optimizer.html

### MariaDB Documentation
- MariaDB Server Documentation: https://mariadb.com/kb/en/
- MariaDB Optimizer: https://mariadb.com/kb/en/optimizer/
- System-Versioned Tables: https://mariadb.com/kb/en/system-versioned-tables/

### Ra Codebase References
- Dialect Support: `/home/gburd/ws/ra/crates/ra-dialect/src/dialect.rs`
- MySQL Metadata: `/home/gburd/ws/ra/crates/ra-metadata/src/mysql.rs`
- MySQL Rules: `/home/gburd/ws/ra/rules/database-specific/mysql/`
- SQL Parser: `/home/gburd/ws/ra/crates/ra-parser/src/sql_to_relexpr.rs`

---

**End of Report**
