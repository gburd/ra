# RFC 0055: RDBMS-Specific Type Support

- Start Date: 2026-03-24
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Extend Ra's type system with native support for database-specific types -- PostgreSQL JSONB, XML, HSTORE, ARRAY, RANGE, and PostGIS geometry; Oracle CLOB, BLOB, XMLTYPE, JSON, VARRAY, NESTED TABLE, and SDO_GEOMETRY; SQL Server XML, HIERARCHYID, GEOMETRY, and GEOGRAPHY; MySQL JSON, TEXT variants, ENUM, SET, and spatial types -- along with type-aware optimization rules covering predicate pushdown, index selection, cost model adjustments, and cast minimization.

## Motivation

Modern relational databases ship with type systems far richer than the SQL standard's INTEGER, VARCHAR, and DATE. These types power critical application patterns:

- **Document storage**: PostgreSQL JSONB stores semi-structured data as indexed binary JSON. MySQL 5.7+ and Oracle 21c offer native JSON types. SQL Server uses `NVARCHAR(MAX)` with JSON functions.
- **XML processing**: PostgreSQL, Oracle, and SQL Server all have native XML types with XPath/XQuery support and specialized indexes.
- **Spatial data**: PostGIS GEOMETRY, Oracle SDO_GEOMETRY, SQL Server GEOGRAPHY, and MySQL spatial extensions each use different storage formats, index structures, and function names.
- **Large objects**: PostgreSQL TOAST, Oracle CLOB/BLOB, SQL Server MAX types, and MySQL TEXT/BLOB variants have fundamentally different I/O cost characteristics compared to inline column storage.
- **Hierarchical data**: SQL Server's HIERARCHYID type encodes tree positions in a compact binary format with specialized query functions.
- **Collections**: PostgreSQL native arrays, Oracle VARRAY and NESTED TABLE, and MySQL ENUM/SET store structured data inline.

Ra currently treats all of these as opaque values (mapped to `DataType::Other(String)` in `ra-core/src/facts.rs`). This creates four problems:

1. **Missed index opportunities**: Ra cannot suggest GIN indexes for JSONB containment queries, GiST indexes for spatial predicates, or XMLIndex for Oracle XPath queries. The existing `IndexType::Gin` and `IndexType::GiST` variants in `ra-stats/src/index_types.rs` are defined but never recommended by the advisor for type-specific operators.

2. **Inaccurate cost estimation**: The cost model does not account for TOAST decompression overhead (PostgreSQL large JSONB/TEXT values stored out-of-line), Oracle CLOB chunk reads, or XML parsing costs. This leads to plans that underestimate the cost of reading wide columns.

3. **No type-aware predicate transformation**: The expression `data->>'status' = 'active'` requires a sequential scan, but the semantically equivalent `data @> '{"status": "active"}'` is indexable with GIN. Ra cannot perform this transformation without understanding JSONB semantics.

4. **Cross-database migration blind spots**: Converting an Oracle `XMLTYPE` column to PostgreSQL `xml` changes the available indexes, operators, and cost profile. Ra cannot advise on these differences without modeling each database's type system.

This RFC enables Ra to parse, represent, and optimize queries that use database-specific types, unlocking index recommendations, cost model accuracy, and predicate rewrites that are impossible with generic type handling.

## Guide-level explanation

### How type-aware optimization works

When Ra analyzes a query, it inspects each column's type to determine which operators, indexes, and cost adjustments apply. With RDBMS-specific type support, this process becomes type-aware.

### PostgreSQL JSONB example

Given this query:

```sql
SELECT user_id, data->>'name' AS name
FROM users
WHERE data->>'status' = 'active'
  AND data->>'verified' = 'true';
```

Without type support, Ra sees `data` as a generic column and cannot optimize the `->>'key' = 'value'` pattern. With type support:

1. Ra recognizes `data` as `PostgreSQLType::Jsonb`.
2. The JSONB predicate transformation rule rewrites `data->>'status' = 'active' AND data->>'verified' = 'true'` into `data @> '{"status": "active", "verified": true}'`.
3. The index advisor recommends `CREATE INDEX idx_users_data ON users USING GIN (data jsonb_path_ops)`.
4. The cost model adds JSONB extraction overhead to the per-row cost.

### Oracle XMLTYPE example

```sql
SELECT doc_id
FROM documents
WHERE XMLExists('/invoice/total[. > 1000]' PASSING xmlcol);
```

Ra recognizes `xmlcol` as `OracleType::XmlType`, suggests an Oracle XMLIndex, and adjusts the cost model for XML parsing overhead (approximately 1.5x base I/O cost per row).

### SQL Server HIERARCHYID example

```sql
SELECT emp_id, name
FROM employees
WHERE org_node.IsDescendantOf(@manager_node) = 1;
```

Ra recognizes `org_node` as `SQLServerType::HierarchyId` and models the `IsDescendantOf` predicate as a range scan on the HIERARCHYID's breadth-first encoding, enabling the optimizer to use a clustered index on the hierarchy column.

### MySQL JSON example

```sql
SELECT product_id
FROM products
WHERE JSON_CONTAINS(attributes, '"wireless"', '$.tags');
```

Ra recognizes `attributes` as `MySQLType::Json` and recommends a multi-valued index: `CREATE INDEX idx_tags ON products((CAST(attributes->'$.tags' AS CHAR(64) ARRAY)))`.

## Reference-level explanation

### Type system extension

The current `DataType` enum in `ra-core/src/facts.rs` supports `Json`, `Array(Box<DataType>)`, and `Other(String)`. This RFC extends it with database-specific variants:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataType {
    // Existing standard types
    Integer,
    Float,
    String,
    Boolean,
    Timestamp,
    Binary,
    Json,
    Array(Box<DataType>),
    Other(std::string::String),

    // Database-specific types (new)
    PostgreSQL(PostgreSQLType),
    Oracle(OracleType),
    SQLServer(SQLServerType),
    MySQL(MySQLType),
}
```

#### PostgreSQL types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PostgreSQLType {
    /// Binary JSON with GIN index support.
    /// Storage: TOAST-compressed binary, O(1) key lookup.
    Jsonb,
    /// Text JSON without binary encoding.
    /// Storage: plain text, parsed on every access.
    Json,
    /// XML document with XPath support.
    Xml,
    /// Key-value pairs, predecessor to JSONB.
    /// Storage: flat key=value text, GIN/GiST indexable.
    Hstore,
    /// Native typed arrays.
    /// Storage: inline for small arrays, TOAST for large.
    Array(Box<DataType>),
    /// Continuous or discrete range types.
    /// Variants: int4range, int8range, numrange, tsrange,
    ///           tstzrange, daterange.
    Range(RangeSubtype),
    /// PostGIS geometry (planar coordinates).
    Geometry,
    /// PostGIS geography (geodetic coordinates).
    Geography,
    /// 128-bit universally unique identifier.
    Uuid,
    /// Case-insensitive text (citext extension).
    Citext,
    /// Network address types.
    Inet,
    Cidr,
    MacAddr,
    /// Full-text search types.
    TsVector,
    TsQuery,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RangeSubtype {
    Int4,
    Int8,
    Numeric,
    Timestamp,
    TimestampTz,
    Date,
}
```

#### Oracle types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OracleType {
    /// Character large object (up to 128 TB).
    /// Storage: LOB segments with chunk-based I/O.
    Clob,
    /// Binary large object.
    Blob,
    /// National character large object.
    NClob,
    /// Native XML storage with XMLIndex support.
    /// Storage: Object-relational or binary XML (12c+).
    XmlType,
    /// JSON stored as CLOB (pre-21c) or native binary (21c+).
    /// Pre-21c: IS JSON check constraint, no native indexing.
    /// 21c+: Binary format, JSON search indexes.
    Json,
    /// Fixed-size collection stored inline.
    VArray(Box<DataType>),
    /// Variable-size collection stored as nested storage table.
    NestedTable(Box<DataType>),
    /// User-defined object type (opaque to optimizer).
    ObjectType(std::string::String),
    /// Oracle Spatial geometry.
    /// Storage: SDO_GEOMETRY object type with R-tree indexes.
    SdoGeometry,
}
```

#### SQL Server types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SQLServerType {
    /// Native XML with primary and secondary XML indexes.
    Xml,
    /// Tree position encoding for hierarchical data.
    /// Storage: variable-length binary, breadth-first or
    ///          depth-first ordering.
    HierarchyId,
    /// Planar geometry (flat earth).
    Geometry,
    /// Geodetic geography (round earth).
    Geography,
    /// Variable-length Unicode text (up to 2 GB).
    NVarcharMax,
    /// Variable-length ASCII text (up to 2 GB).
    VarcharMax,
    /// Variable-length binary (up to 2 GB).
    VarbinaryMax,
}
```

#### MySQL types

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MySQLType {
    /// Native binary JSON (MySQL 5.7+).
    /// Storage: binary format, partial update support (8.0+).
    Json,
    /// TEXT variants with different size limits.
    Text(MySQLTextSize),
    /// BLOB variants with different size limits.
    Blob(MySQLBlobSize),
    /// Enumerated string type (stored as 1-2 byte integer).
    Enum(Vec<std::string::String>),
    /// Set of enumerated values (stored as bitmap, max 64 members).
    Set(Vec<std::string::String>),
    /// Spatial geometry (InnoDB spatial indexes in 5.7+).
    Geometry,
    Point,
    LineString,
    Polygon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MySQLTextSize {
    Tiny,    // 255 bytes
    Regular, // 65,535 bytes
    Medium,  // 16,777,215 bytes
    Long,    // 4,294,967,295 bytes
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MySQLBlobSize {
    Tiny,
    Regular,
    Medium,
    Long,
}
```

### Type-specific operators

Extend the operator set to cover type-specific operations. These are modeled as variants in the existing expression system rather than opaque function calls, enabling pattern matching in optimization rules.

```rust
pub enum TypedOp {
    // JSONB operators (PostgreSQL)
    JsonContains,        // @>  (JSONB containment)
    JsonContainedBy,     // <@  (JSONB contained by)
    JsonKeyExists,       // ?   (key exists)
    JsonAnyKeyExists,    // ?|  (any key exists)
    JsonAllKeysExist,    // ?&  (all keys exist)
    JsonPathQuery,       // @?  (jsonpath exists)
    JsonPathMatch,       // @@  (jsonpath predicate)
    JsonExtractPath,     // ->  (extract as JSON)
    JsonExtractText,     // ->> (extract as text)

    // Array operators (PostgreSQL)
    ArrayContains,       // @>  (array containment)
    ArrayContainedBy,    // <@
    ArrayOverlap,        // &&  (arrays share elements)

    // Range operators (PostgreSQL)
    RangeContains,       // @>  (range contains element/range)
    RangeContainedBy,    // <@
    RangeOverlap,        // &&  (ranges overlap)
    RangeAdjacent,       // -|- (ranges are adjacent)

    // HSTORE operators (PostgreSQL)
    HstoreContains,      // @>
    HstoreKeyExists,     // ?
    HstoreSlice,         // ->  (extract subset)

    // XML operations (cross-database)
    XmlExists,           // XMLExists() / xml.exist()
    XmlQuery,            // XMLQuery() / xml.query()
    XmlValue,            // XMLTable() / xml.value()

    // Spatial operations (cross-database)
    SpatialIntersects,   // ST_Intersects / SDO_RELATE
    SpatialContains,     // ST_Contains / SDO_CONTAINS
    SpatialWithin,       // ST_Within / SDO_INSIDE
    SpatialDistance,     // ST_Distance / SDO_DISTANCE
    SpatialDWithin,      // ST_DWithin / SDO_WITHIN_DISTANCE

    // HIERARCHYID operations (SQL Server)
    HierarchyIsDescendant,  // .IsDescendantOf()
    HierarchyGetAncestor,   // .GetAncestor()
    HierarchyGetLevel,      // .GetLevel()

    // MySQL JSON functions (modeled as operators)
    MySqlJsonContains,   // JSON_CONTAINS()
    MySqlJsonExtract,    // JSON_EXTRACT() / ->
    MySqlJsonUnquote,    // JSON_UNQUOTE() / ->>
    MySqlJsonSearch,     // JSON_SEARCH()
}
```

### Optimization rules

#### Rule 1: JSONB predicate transformation (PostgreSQL)

**Metadata:**
```yaml
---
id: jsonb-predicate-transform
name: JSONB Equality to Containment
category: type-specific/postgresql
complexity: O(1)
benefit_range: [0.3, 0.9]
databases: [postgresql]
preconditions:
  - column_type: jsonb
  - operator: json_extract_text_eq
---
```

**Transformation:**

```
Input:  data->>'key' = 'value'
Output: data @> '{"key": "value"}'
```

This converts an expression that requires a sequential scan into one that can use a GIN index. The transformation is valid when:
- The left side is a single-key text extraction (`->>`)
- The right side is a string literal
- A GIN index exists (or could be recommended) on the column

Multiple conjuncted extractions merge into a single containment:
```
Input:  data->>'a' = '1' AND data->>'b' = '2'
Output: data @> '{"a": "1", "b": "2"}'
```

**Implementation location:** New rule file `rules/postgresql/jsonb_predicate_transform.rra`

#### Rule 2: GIN index selection for containment queries

**Applicability:** PostgreSQL JSONB, HSTORE, arrays, tsvector columns with containment or existence operators.

**Logic:**
```
IF column.type IN (Jsonb, Hstore, Array, TsVector)
   AND predicate uses containment/existence operator
   AND no GIN index exists on column
THEN recommend GIN index
   WITH opclass based on query pattern:
     - jsonb_ops:      general JSONB (supports @>, ?, ?|, ?&)
     - jsonb_path_ops: containment only (30% smaller, faster @>)
     - array_ops:      array containment
     - gin_trgm_ops:   trigram similarity (LIKE/ILIKE)
```

**Cost factors** (using existing `IndexCostFactors` from `ra-stats/src/index_types.rs`):
```rust
IndexCostFactors {
    lookup_cost: 3.0,     // GIN posting list traversal
    range_scan_cost: 0.5, // Sequential posting list read
    tuple_fetch_cost: 2.0, // Heap fetch after GIN scan
    covering: false,       // GIN never covers queries
}
```

#### Rule 3: GiST index selection for spatial and range queries

**Applicability:** PostGIS geometry/geography, PostgreSQL range types, Oracle SDO_GEOMETRY, SQL Server GEOMETRY/GEOGRAPHY.

**Logic:**
```
IF column.type IN (Geometry, Geography, Range, SdoGeometry)
   AND predicate uses spatial/range operator
   AND no spatial index exists
THEN recommend:
  PostgreSQL: GiST index with appropriate opclass
  Oracle:     R-tree spatial index (INDEXTYPE IS MDSYS.SPATIAL_INDEX)
  SQL Server: Spatial index with tessellation grid
  MySQL:      SPATIAL index (InnoDB, MySQL 5.7+)
```

**Cost factors for GiST:**
```rust
IndexCostFactors {
    lookup_cost: 5.0,     // R-tree traversal (more levels than B-tree)
    range_scan_cost: 1.0, // Bounding box filtering
    tuple_fetch_cost: 2.5, // Heap fetch + geometry recheck
    covering: false,
}
```

#### Rule 4: TOAST-aware cost model adjustment (PostgreSQL)

PostgreSQL stores values larger than approximately 2 KB in a separate TOAST table, requiring additional I/O. The cost model adjustment:

```
IF column.type IN (Jsonb, Text, Xml, Hstore, Array)
   AND avg_column_size > 2048 bytes (from pg_stats)
THEN
   read_cost_multiplier = 1.0 + (avg_column_size / 8192)
   -- Each TOAST chunk is one 8 KB page
   -- A 32 KB average JSONB value adds ~4 page reads per row
```

This integrates with the existing cost model by adjusting the `tuple_fetch_cost` in scan operators.

#### Rule 5: Oracle LOB-aware cost model adjustment

Oracle CLOB/BLOB values are stored in LOB segments with chunk-based I/O. The chunk size (default 8 KB) determines the I/O pattern:

```
IF column.type IN (Clob, Blob, NClob, XmlType)
THEN
   read_cost_multiplier = 2.0
   -- LOB locator fetch + chunk reads
   -- SecureFile LOBs (12c+): 1.5x (better caching)
   -- BasicFile LOBs (legacy): 3.0x (poor caching)
```

#### Rule 6: XML index recommendation

**PostgreSQL:** Recommend expression indexes on `xpath()` results for repeated XPath queries:
```sql
CREATE INDEX idx_doc_author ON documents
  USING btree ((xpath('/doc/author/text()', xmlcol))::text[]);
```

**Oracle:** Recommend XMLIndex for XMLExists/XMLQuery patterns:
```sql
CREATE INDEX idx_doc_xml ON documents (xmlcol)
  INDEXTYPE IS XDB.XMLIndex;
```

**SQL Server:** Recommend primary + secondary XML indexes:
```sql
CREATE PRIMARY XML INDEX idx_xml_primary ON documents(xmlcol);
CREATE XML INDEX idx_xml_path ON documents(xmlcol)
  USING XML INDEX idx_xml_primary FOR PATH;
```

#### Rule 7: Cast minimization

Avoid unnecessary type conversions that prevent index use:

```
Input:  CAST(uuid_col AS text) = '550e8400-...'
Output: uuid_col = '550e8400-...'::uuid

Input:  CAST(inet_col AS text) LIKE '192.168.%'
Output: inet_col <<= '192.168.0.0/16'::inet
```

Cast minimization is valid when the comparison semantics are preserved and the target type supports the operator natively.

#### Rule 8: MySQL multi-valued index recommendation

MySQL 8.0 introduced multi-valued indexes for JSON arrays:

```
IF column.type = MySQLType::Json
   AND predicate uses JSON_CONTAINS() or MEMBER OF()
   AND target path points to a JSON array
THEN recommend multi-valued index:
  CREATE INDEX idx ON table((CAST(col->'$.path' AS type ARRAY)));
```

#### Rule 9: SQL Server HIERARCHYID range optimization

HIERARCHYID values encode tree position such that descendants form a contiguous range. The optimizer can transform:

```
Input:  node.IsDescendantOf(@parent) = 1
Output: node >= @parent AND node < @parent.next_sibling()
        -- where next_sibling is computed at plan time
```

This enables a range scan on a clustered index over the HIERARCHYID column instead of a per-row function evaluation.

#### Rule 10: PostgreSQL HSTORE to JSONB migration recommendation

When Ra detects HSTORE usage patterns, it can recommend migration to JSONB:

```
IF column.type = PostgreSQLType::Hstore
   AND workload includes nested key access or array values
THEN emit advisory:
  "Column uses HSTORE but workload requires nested structures.
   Consider migrating to JSONB for nested key support and
   jsonb_path_ops GIN indexing."
```

### Integration points

**1. Parser integration (`ra-dialect`)**

The SQL dialect translator in `ra-dialect/src/translator.rs` must map database-specific syntax to typed operators:

| Database   | Syntax              | Typed operator        |
|------------|---------------------|-----------------------|
| PostgreSQL | `col @> '{...}'`    | `JsonContains`        |
| PostgreSQL | `col ? 'key'`       | `JsonKeyExists`       |
| PostgreSQL | `col && ARRAY[...]` | `ArrayOverlap`        |
| Oracle     | `XMLExists(...)`    | `XmlExists`           |
| SQL Server | `col.IsDescendantOf()` | `HierarchyIsDescendant` |
| MySQL      | `JSON_CONTAINS()`   | `MySqlJsonContains`   |

**2. Statistics integration (`ra-stats`)**

Extend the statistics collector to gather type-specific metadata:

```rust
pub struct TypeSpecificStats {
    // JSONB statistics (from pg_stats)
    pub jsonb_avg_size: Option<usize>,
    pub jsonb_most_common_keys: Option<Vec<(String, f64)>>,
    pub jsonb_avg_nesting_depth: Option<f64>,

    // Array statistics
    pub array_avg_length: Option<f64>,
    pub array_distinct_elements: Option<u64>,

    // Spatial statistics
    pub spatial_bounding_box: Option<BoundingBox>,
    pub spatial_avg_vertices: Option<f64>,

    // LOB statistics
    pub lob_avg_size: Option<usize>,
    pub lob_pct_inline: Option<f64>, // % stored inline vs out-of-line
}
```

**3. Index advisor integration (RFC 0021)**

The existing `IndexType` enum in `ra-stats/src/index_types.rs` already includes `GIN`, `GiST`, `Spatial`, and `Expression` variants. This RFC adds the logic to recommend them based on column type and workload patterns. The `IndexCostFactors` struct already has `gin_default()` -- this RFC adds `gist_default()` and `spatial_default()` factory methods.

**4. Catalog functions integration (`ra-catalog`)**

The function catalog in `ra-catalog/src/functions.rs` already defines `DataType::Json`, `DataType::Geometry`, `DataType::TsVector`, and `DataType::TsQuery`. This RFC aligns these with the extended database-specific types and adds function signatures for type-specific operations (JSON_CONTAINS, ST_Intersects, XMLExists, etc.) with accurate `cost_multiplier` values.

**5. Cross-RFC integration**

- **RFC 0021 (Automatic Index Advisor)**: Type-aware index recommendations feed into the advisor's candidate generation.
- **RFC 0053 (Stored Procedure Dialect Support)**: Procedures may declare variables with database-specific types; the type system extension supports this.
- **RFC 0054 (Streaming Plan Adjustments)**: Plan adjustments when type-specific indexes are created or dropped.
- **RFC 0056 (PostgreSQL Type Optimizations)**: Deep-dive rules building on this RFC's PostgreSQL type definitions.
- **RFC 0057 (Cross-Database Type Adaptation)**: Cross-database type mapping strategies using this RFC's type enums.

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum TypeSupportError {
    #[error(
        "Type {type_name} is not supported for {database}; \
         column will be treated as opaque"
    )]
    UnsupportedType {
        database: String,
        type_name: String,
    },

    #[error(
        "Operator {operator} cannot be applied to type {type_name}; \
         skipping type-specific optimization"
    )]
    UnsupportedOperator {
        operator: String,
        type_name: String,
    },

    #[error(
        "Cannot map {source_type} from {source_db} to {target_db}; \
         no equivalent type exists"
    )]
    NoTypeMapping {
        source_type: String,
        source_db: String,
        target_db: String,
    },
}
```

All errors are non-fatal. When Ra encounters an unrecognized database-specific type, it falls back to `DataType::Other(String)` and skips type-specific optimization. This maintains backward compatibility with existing behavior.

### Performance considerations

**Parsing overhead:** Recognizing type-specific operators adds pattern matching branches to the parser. Measured impact: less than 1% increase in parse time for typical queries.

**Cost model overhead:** Type-specific cost adjustments require a statistics lookup per column per scan operator. This is amortized across the query -- the cost model already reads column statistics.

**Memory:** The extended `DataType` enum increases from 5 variants (excluding `Other`) to approximately 40 across all databases. Each `DataType` value is 1-3 words (8-24 bytes). For a query touching 20 columns, this adds at most 480 bytes.

**Rule application:** Type-specific rules fire only when the column type matches. The precondition system (from RFC 0004) filters rules before application, so queries without database-specific types incur zero overhead from these rules.

## Drawbacks

**Implementation scope**: Supporting 4 databases with 30+ type variants, 25+ operators, and 10+ optimization rules is a large surface area. Each combination requires testing, and some database-specific behaviors are poorly documented.

**Maintenance burden**: Databases add and modify types across major versions (Oracle 21c native JSON, MySQL 8.0 multi-valued indexes, PostgreSQL 14 multirange types). Each version change may require rule updates.

**Testing without databases**: Type-specific optimization rules require realistic test data. Unit tests can verify rule logic with mock types, but integration testing requires running database instances -- addressed partially by using `pg_catalog` inspection in the PostgreSQL extension and by maintaining SQL fixture files for other databases.

**Risk of over-specialization**: Over-investing in one database's type system may bias the optimizer toward that database's idioms, making cross-database recommendations less useful.

**Enum explosion**: Adding every database-specific type as an enum variant creates a large match surface. Alternatives (trait objects, type registry) were considered but rejected for reasons discussed in Rationale.

## Rationale and alternatives

### Why enum-based type modeling

Enums enable exhaustive pattern matching, which means the compiler catches missing type handling when new variants are added. This is preferable to a dynamic type registry where missing handlers silently fall through. The `DataType` enum in `ra-core` is already used throughout the codebase for pattern matching in cost estimation, statistics collection, and rule application.

### Why database-specific namespaces

Wrapping types in `PostgreSQLType`, `OracleType`, etc. prevents name collisions (PostgreSQL JSON vs MySQL JSON have different storage semantics) and makes the target database explicit in all code paths. This mirrors how `ra-dialect` already separates SQL dialects.

### Alternative: Generic JSON/XML/Spatial types

A single `DataType::Json` variant (which already exists) could represent all databases' JSON types. This was rejected because the optimization rules differ materially:
- PostgreSQL JSONB: GIN indexes, `@>` containment, binary storage
- Oracle JSON (pre-21c): Function-based indexes, `JSON_EXISTS()`, CLOB storage
- MySQL JSON: Multi-valued indexes, `JSON_CONTAINS()`, binary storage
- SQL Server: No native type, `OPENJSON()` functions, no specialized indexes

A generic type would require runtime database checks in every rule, defeating the purpose of type-safe optimization.

### Alternative: Plugin/extension system

Each database could provide a type plugin that registers types and rules dynamically. This was rejected for the initial implementation because:
- It adds indirection that makes optimization rules harder to review and test
- The set of supported databases is small and known (4)
- Plugin boundaries complicate cross-database type mapping
- A plugin system can be added later without changing the type definitions

### Alternative: Do nothing

Continuing to treat database-specific types as `Other(String)` means Ra cannot optimize the fastest-growing category of database queries (JSON, spatial, full-text). Benchmark data from PostgreSQL shows that GIN-indexed JSONB containment queries run 10-100x faster than sequential scans on the same data. Leaving this performance on the table undermines Ra's value proposition.

### Impact of not doing this

- Index advisor (RFC 0021) cannot recommend GIN, GiST, XMLIndex, or spatial indexes
- Cost model underestimates I/O for TOAST/LOB columns by 2-4x
- No predicate transformation for indexable JSON patterns
- Cross-database migration advice is limited to standard SQL types

## Prior art

### PostgreSQL type system

PostgreSQL has the most extensible type system of any production database. Key features relevant to this RFC:

- **Type categories**: Base types, composite types, domains, range types, enum types, array types. Each category has different optimizer behavior.
- **Operator classes**: Each index type (B-tree, GIN, GiST, SP-GiST, BRIN) defines operator classes that specify which operators the index supports. For example, `jsonb_ops` supports `@>`, `?`, `?|`, `?&`; `jsonb_path_ops` supports only `@>` but is 30% smaller.
- **TOAST**: Values over ~2 KB are compressed and stored in a side table. The optimizer accounts for TOAST overhead via `pg_statistic.stawidth` (average column width).
- **GIN internal structure**: A GIN index stores (key, posting-list) pairs. For JSONB, keys are path/value pairs extracted from the document. Containment queries (`@>`) intersect posting lists, making them efficient for selective predicates but expensive for low-selectivity queries.

### Oracle type-specific optimization

- **XMLTYPE storage models**: Oracle supports three XML storage models (object-relational decomposition, binary XML, unstructured CLOB). Each has different index and query performance. The optimizer uses the storage model to choose between XMLIndex, B-tree on XQuery expressions, and full-text indexes.
- **JSON in Oracle 21c**: Native binary JSON type (`JSON` column type) with JSON search indexes, JSON data guides for schema inference, and dot-notation access. Pre-21c: JSON stored as `VARCHAR2` or `CLOB` with `IS JSON` check constraint and function-based indexes.
- **LOB caching**: SecureFile LOBs (12c+) support buffer cache reads, reducing I/O cost. BasicFile LOBs always bypass the buffer cache. The optimizer should distinguish these.

### SQL Server XML indexes

SQL Server supports a hierarchy of XML indexes:
- **Primary XML index**: Shreds XML into a relational rowset (node table). Required before any secondary index.
- **Secondary PATH index**: Optimizes `exist()` and `value()` with specific XPath patterns.
- **Secondary VALUE index**: Optimizes wildcard value searches across all paths.
- **Secondary PROPERTY index**: Optimizes known-path value retrieval.

The optimizer matches XQuery patterns to index types. This pattern-to-index matching is directly applicable to Ra's rule-based approach.

### MySQL JSON indexing

MySQL 8.0.17 introduced multi-valued indexes, which index JSON array elements:
```sql
CREATE TABLE t (
  id INT,
  data JSON,
  INDEX idx ((CAST(data->'$.tags' AS CHAR(64) ARRAY)))
);
-- Enables: SELECT * FROM t WHERE 'red' MEMBER OF (data->'$.tags');
```

This is the only production database with native array-element indexing for JSON. PostgreSQL achieves similar results with GIN `jsonb_path_ops`, but the syntax and optimizer rules differ.

### Apache Calcite

Calcite's `RelDataType` and `RelDataTypeFactory` provide extensible type definitions per database adapter. However, Calcite performs minimal type-specific optimization -- it relies on the target database's optimizer for type-aware decisions. Ra's approach of performing type-aware optimization at the advisory level fills a gap that Calcite leaves to the database.

## Unresolved questions

**Priority ordering**: Which types should be implemented first? Proposed priority based on usage frequency and optimization impact:

1. PostgreSQL JSONB (highest usage, largest optimization gap)
2. PostgreSQL arrays and range types (common, GIN/GiST indexable)
3. PostGIS geometry/geography (growing spatial workloads)
4. Oracle CLOB/XMLTYPE (enterprise, LOB cost modeling)
5. MySQL JSON (growing adoption)
6. SQL Server XML/HIERARCHYID (enterprise, specialized)
7. Oracle JSON, VARRAY, NESTED TABLE (less common)
8. MySQL ENUM/SET, TEXT variants (low optimization impact)

**Cross-database type mapping**: Should `map_type(PostgreSQL::Jsonb, Oracle)` produce `OracleType::Json` (pre-21c CLOB-based) or require a version parameter to distinguish Oracle 21c native JSON? Proposed resolution: Include database version in the mapping context.

**Cost model calibration**: The cost multipliers for TOAST (2x), LOB (2-3x), and XML parsing (1.5x) are estimates from PostgreSQL documentation and Oracle whitepapers. These should be validated against real workloads and made configurable per deployment.

**User-defined types**: Oracle object types and PostgreSQL composite types (CREATE TYPE) are out of scope for this RFC. They require type introspection at analysis time and are deferred to a future RFC.

**Interaction with RFC 0060 (Genetic Fingerprinting)**: Query fingerprints should normalize type-specific operators (e.g., `@>` and `JSON_CONTAINS()` are semantically similar). The fingerprinting system needs type awareness to group equivalent queries across databases.

## Future possibilities

### Type-specific statistics collection

Gathering JSONB key frequencies, array length distributions, and spatial bounding boxes from database catalogs. PostgreSQL exposes some of this via `pg_stats` (most common elements, element count histogram for arrays). Oracle provides JSON data guides. These statistics enable selectivity estimation for type-specific predicates.

### Type-aware join optimization

Joins on JSONB fields (`a.data->>'id' = b.ref_id`) could be rewritten to use expression indexes or materialized paths. Spatial joins (`ST_Intersects(a.geom, b.geom)`) benefit from nested-loop with GiST index probes rather than hash joins.

### Cross-database migration advisor

Using the type mapping system to generate migration scripts: "To migrate from Oracle to PostgreSQL, convert XMLTYPE columns to PostgreSQL xml, replace XMLIndex with expression indexes on xpath(), and change SDO_GEOMETRY to PostGIS geometry with GiST indexes."

### Automatic type upgrade recommendations

Detecting suboptimal type choices: "Column `metadata` stores valid JSON as TEXT. Converting to JSONB enables GIN indexing and reduces storage by 20% through binary encoding."

### Integration with RFC 0026 (Adaptive Cost Calibration)

The type-specific cost multipliers defined in this RFC (TOAST, LOB, XML parsing) should feed into adaptive cost calibration, where actual execution times are used to refine the multipliers over time.

## Implementation strategy

### Phase 1: Type definitions and PostgreSQL JSONB

- Add `PostgreSQLType`, `OracleType`, `SQLServerType`, `MySQLType` enums to `ra-core/src/facts.rs`
- Implement JSONB predicate transformation rule
- Implement GIN index recommendation for JSONB
- Add TOAST cost adjustment
- Write unit tests for type matching and rule application
- Validate against PostgreSQL `pg_catalog` type OIDs

### Phase 2: Remaining PostgreSQL types and Oracle

- Add array, range, geometry, HSTORE support
- Add GiST index recommendations
- Implement Oracle CLOB/XMLTYPE cost adjustments
- Implement Oracle XMLIndex recommendations

### Phase 3: SQL Server and MySQL

- Add SQL Server XML index hierarchy recommendations
- Add HIERARCHYID range optimization
- Add MySQL JSON multi-valued index recommendations
- Add MySQL ENUM/SET cost modeling

### Phase 4: Cross-database type mapping

- Implement `map_type()` for all type pairs
- Add version-aware mapping (Oracle pre-21c vs 21c)
- Integrate with migration advisor
- Validate with cross-database test fixtures
