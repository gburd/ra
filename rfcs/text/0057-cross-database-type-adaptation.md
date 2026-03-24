# RFC 0057: Cross-Database Type Storage Adaptation

- Start Date: 2026-03-24
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

The same logical type (e.g., "JSON", "XML", "spatial geometry") is stored and indexed
differently across databases, with performance implications spanning 10-100x. Ra's
optimizer must adapt its cost model, query rewriting, and index recommendations based
on the target database's storage format for each type. This RFC defines how the optimizer
detects storage characteristics, adjusts cost estimates, and selects optimization
strategies per database for JSON, XML, and spatial types.

## Motivation

Database vendors implement the same logical types using fundamentally different storage
strategies. These differences directly affect:

1. **Query performance**: PostgreSQL JSONB containment queries with GIN indexes execute
   in O(log N); the same logical operation on Oracle JSON (CLOB) requires full-text
   parsing at O(N) per row without a function-based index.

2. **Index selection**: A GIN index on PostgreSQL JSONB supports arbitrary key lookup.
   Oracle requires a separate function-based index per JSON path. MySQL 8.0+ supports
   multi-valued indexes. SQL Server requires computed columns with B-tree indexes.
   Recommending the wrong index type wastes storage and provides no benefit.

3. **Query rewriting**: PostgreSQL uses `@>` for JSON containment, Oracle uses
   `JSON_EXISTS()`, MySQL uses `JSON_CONTAINS()`, SQL Server uses `OPENJSON()` with
   `EXISTS`. The optimizer must generate the correct syntax and choose the form that
   enables index usage on each platform.

4. **Cost estimation accuracy**: Without storage-aware cost models, the optimizer
   produces plans that are optimal for one database but catastrophic for another.
   A plan that assumes O(1) JSON key lookup (PostgreSQL JSONB) will underestimate
   Oracle JSON costs by 10-50x.

### Concrete Impact

Consider a table with 10 million rows containing a JSON column `data`, queried by:

```sql
SELECT id FROM orders WHERE data contains {"status": "shipped"}
```

| Database    | Storage Format     | With Index       | Without Index   | Ratio vs PG |
|-------------|--------------------|------------------|-----------------|-------------|
| PostgreSQL  | JSONB (binary)     | 0.5ms (GIN)      | 2,800ms (scan)  | 1x          |
| MySQL 8.0   | Binary JSON        | 1.2ms (MVI)      | 3,200ms (scan)  | 2.4x        |
| Oracle 21c  | CLOB + IS JSON     | 8ms (func-based) | 45,000ms (scan) | 16x         |
| SQL Server  | NVARCHAR(MAX)      | 5ms (computed)   | 38,000ms (scan) | 10x         |

The optimizer that treats these as equivalent will produce incorrect cost estimates
and suboptimal plans for 3 out of 4 databases.

## Guide-level explanation

### How Storage Adaptation Works

Ra maintains a **storage profile** for each (database, logical type) pair. When the
optimizer encounters a column of a recognized type, it looks up the storage profile
to determine:

- **Physical representation**: Binary, CLOB, native XML, etc.
- **Available index types**: GIN, GiST, function-based, multi-valued, XML indexes
- **Cost multiplier**: Relative performance compared to the fastest implementation
- **Preferred query form**: Which syntax enables index usage on this platform

### Example: JSON Query Across Databases

**Logical intent** (database-agnostic):

```sql
SELECT id, data FROM users WHERE data contains {"status": "active"}
```

**PostgreSQL (JSONB -- binary format):**

```sql
-- GIN index enables O(log N) containment check
SELECT id, data FROM users WHERE data @> '{"status": "active"}';

-- Recommended index:
CREATE INDEX idx_users_data ON users USING GIN (data);
-- Cost: ~0.5ms per query (10M rows, indexed)
```

**Oracle 21c (CLOB with IS JSON constraint):**

```sql
-- Function-based index on specific path
SELECT id, data FROM users
WHERE JSON_EXISTS(data, '$.status?(@ == "active")');

-- Recommended index:
CREATE INDEX idx_users_status
  ON users (JSON_VALUE(data, '$.status'));
-- Cost: ~8ms per query (10M rows, indexed)
-- Without index: ~45,000ms (full CLOB parse per row)
```

**MySQL 8.0 (binary JSON format):**

```sql
-- Multi-valued index on JSON array, or expression index on path
SELECT id, data FROM users
WHERE JSON_CONTAINS(data, '"active"', '$.status');

-- Recommended index (expression index):
CREATE INDEX idx_users_status
  ON users ((CAST(data->>'$.status' AS CHAR(50))));
-- Cost: ~1.2ms per query (10M rows, indexed)
```

**SQL Server 2022 (NVARCHAR(MAX) with JSON functions):**

```sql
-- Computed column + B-tree index
ALTER TABLE users ADD status_computed
  AS JSON_VALUE(data, '$.status');
CREATE INDEX idx_users_status ON users (status_computed);

SELECT id, data FROM users WHERE JSON_VALUE(data, '$.status') = 'active';
-- Cost: ~5ms per query (10M rows, indexed)
```

Ra adapts its optimization strategy per database, selecting the query form that
leverages available indexes and adjusting cost estimates to reflect actual storage
performance.

### Example: Spatial Query Across Databases

**Logical intent:**

```sql
SELECT name FROM buildings WHERE location within radius(47.6, -122.3, 1000m)
```

**PostgreSQL + PostGIS:**

```sql
SELECT name FROM buildings
WHERE ST_DWithin(location::geography, ST_MakePoint(-122.3, 47.6)::geography, 1000);
-- GiST index: ~2ms (10M rows)
-- Recommended: CREATE INDEX idx_loc ON buildings USING GIST (location);
```

**Oracle Spatial:**

```sql
SELECT name FROM buildings
WHERE SDO_WITHIN_DISTANCE(location, SDO_GEOMETRY(2001, 4326,
  SDO_POINT_TYPE(-122.3, 47.6, NULL), NULL, NULL), 'distance=1000 unit=M') = 'TRUE';
-- R-tree index: ~5ms (10M rows)
-- Requires: INSERT INTO USER_SDO_GEOM_METADATA ...
```

**SQL Server:**

```sql
SELECT name FROM buildings
WHERE location.STDistance(geography::Point(47.6, -122.3, 4326)) <= 1000;
-- Grid-based spatial index: ~8ms (10M rows)
-- Requires: CREATE SPATIAL INDEX with grid densities
```

### Example: XML Query Across Databases

**Logical intent:**

```sql
SELECT id FROM documents WHERE xml_data contains element <status>active</status>
```

**PostgreSQL (native XML type):**

```sql
SELECT id FROM documents
WHERE (xpath('//status/text()', xml_data))[1]::text = 'active';
-- No dedicated XML index; must use functional index or full-text search
-- Cost: ~15ms (100K rows, functional B-tree index on xpath expression)
```

**Oracle (XMLTYPE with binary storage):**

```sql
SELECT id FROM documents
WHERE XMLExists('$doc/root/status[text()="active"]'
  PASSING xml_data AS "doc");
-- XMLIndex: ~1ms (100K rows, indexed)
-- Binary XML storage: parsed once at insert, queries operate on binary tree
```

**SQL Server (native XML with indexes):**

```sql
SELECT id FROM documents
WHERE xml_data.exist('/root/status[text()="active"]') = 1;
-- Primary + secondary XML index: ~2ms (100K rows)
-- Secondary PATH index optimizes specific element lookups
```

In this case Oracle and SQL Server have stronger XML optimization than PostgreSQL.
Ra's cost model reflects this -- the typical assumption that PostgreSQL is fastest
does not hold for XML workloads.

## Reference-level explanation

### Core Data Structures

```rust
/// Storage profile for a (database, logical type) pair.
#[derive(Debug, Clone)]
pub struct TypeStorageProfile {
    pub logical_type: LogicalType,
    pub database: DatabaseVariant,
    pub physical_format: PhysicalFormat,
    pub index_capabilities: Vec<IndexCapability>,
    pub base_cost_multiplier: f64,
    pub index_cost_multiplier: f64,
    pub parse_overhead: ParseOverhead,
}

#[derive(Debug, Clone, Copy)]
pub enum LogicalType {
    Json,
    Xml,
    Spatial,
    Array,
    LargeObject,
}

#[derive(Debug, Clone, Copy)]
pub enum DatabaseVariant {
    PostgreSQL,
    Oracle,
    MySQL,
    SQLServer,
}

/// How the database physically stores a logical type.
#[derive(Debug, Clone)]
pub enum PhysicalFormat {
    // JSON variants
    BinaryJson,          // PostgreSQL JSONB, MySQL 8.0 JSON
    ClobJson,            // Oracle JSON (CLOB + IS JSON constraint)
    TextJson,            // SQL Server (NVARCHAR(MAX) with JSON funcs)

    // XML variants
    NativeXmlNoIndex,    // PostgreSQL XML (xpath, no XML indexes)
    BinaryXml,           // Oracle XMLTYPE (binary XML storage)
    ClobXml,             // Oracle XMLTYPE (CLOB storage, older default)
    IndexedXml,          // SQL Server XML (primary + secondary indexes)

    // Spatial variants
    PostGisNative,       // PostGIS GEOMETRY/GEOGRAPHY
    OracleSdo,           // Oracle SDO_GEOMETRY
    SqlServerSpatial,    // SQL Server GEOMETRY/GEOGRAPHY
    MySqlSpatial,        // MySQL GEOMETRY (InnoDB spatial, 8.0+)

    // Large objects
    Toast,               // PostgreSQL TOAST (transparent compression)
    OracleLob,           // Oracle CLOB/BLOB (LOB storage)
    SqlServerMax,        // SQL Server VARCHAR(MAX)/VARBINARY(MAX)
}

/// Parse overhead per row access.
#[derive(Debug, Clone, Copy)]
pub enum ParseOverhead {
    None,             // Binary formats -- already parsed at insert
    PerAccess(f64),   // Cost added per row access (e.g., CLOB JSON parse)
    OnceAtInsert,     // Parsed at write time, reads are fast
}

/// Index capability for a storage format.
#[derive(Debug, Clone)]
pub struct IndexCapability {
    pub index_type: StorageIndexType,
    pub supports_containment: bool,
    pub supports_path_query: bool,
    pub requires_expression: bool,
    pub setup_complexity: SetupComplexity,
}

#[derive(Debug, Clone, Copy)]
pub enum StorageIndexType {
    Gin,                // PostgreSQL GIN (inverted)
    Gist,               // PostgreSQL GiST (generalized search tree)
    BTree,              // Standard B-tree (all databases)
    FunctionBased,      // Oracle function-based index
    MultiValued,        // MySQL 8.0 multi-valued index
    XmlPrimary,         // SQL Server primary XML index
    XmlSecondary,       // SQL Server secondary XML index (PATH, VALUE, PROPERTY)
    OracleXmlIndex,     // Oracle XMLIndex
    SpatialRTree,       // Oracle R-tree spatial index
    SpatialGrid,        // SQL Server grid-based spatial index
    ComputedColumn,     // SQL Server computed column + B-tree
}

#[derive(Debug, Clone, Copy)]
pub enum SetupComplexity {
    Simple,             // CREATE INDEX ... USING GIN (col)
    Moderate,           // Requires expression or computed column
    Complex,            // Requires metadata registration (Oracle Spatial)
}
```

### JSON Storage Adaptation

#### PostgreSQL JSONB

PostgreSQL stores JSON in a decomposed binary format (JSONB). The value is parsed
once at insert time and stored as a binary tree. Key lookups, containment checks,
and path traversals operate directly on the binary representation without reparsing.

**Index support:**
- GIN index on the entire JSONB column: supports `@>` (containment), `?` (key exists),
  `?|` (any key exists), `?&` (all keys exist)
- GIN with `jsonb_path_ops`: smaller index, supports only `@>` containment
- B-tree index on extracted values: `CREATE INDEX ON t ((data->>'key'))`

**Cost characteristics:**
- Key extraction (`->>`): O(log K) where K = number of keys in document
- Containment (`@>`): O(1) with GIN index, O(N * K) without
- Full document access: O(1) (no parsing needed)

```rust
fn json_profile_postgresql() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Json,
        database: DatabaseVariant::PostgreSQL,
        physical_format: PhysicalFormat::BinaryJson,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::Gin,
                supports_containment: true,
                supports_path_query: true,
                requires_expression: false,
                setup_complexity: SetupComplexity::Simple,
            },
            IndexCapability {
                index_type: StorageIndexType::BTree,
                supports_containment: false,
                supports_path_query: true,
                requires_expression: true, // ON t ((data->>'key'))
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 1.0,
        index_cost_multiplier: 0.01, // GIN: ~100x speedup
        parse_overhead: ParseOverhead::None,
    }
}
```

#### Oracle JSON (CLOB Storage)

Oracle stores JSON as CLOB (Character Large Object) by default. A `CHECK (data IS JSON)`
constraint validates format but does not change storage. Every query that accesses a
JSON value must parse the CLOB text into a DOM, extract the requested path, then
discard the parsed structure. Oracle 21c introduced a binary JSON format
(`JSON` data type) but CLOB remains the default for compatibility.

**Index support:**
- Function-based index on `JSON_VALUE(col, '$.path')`: supports equality on one path
- Full-text index on CLOB: supports keyword search but not structured JSON queries
- No equivalent to PostgreSQL's GIN containment index

**Cost characteristics:**
- Path extraction (`JSON_VALUE`): O(D) where D = document size (full parse)
- Containment (`JSON_EXISTS` with path predicate): O(D) per row without index
- Full document access: O(1) (CLOB is read directly)
- With function-based index: O(log N) but only for the indexed path

```rust
fn json_profile_oracle() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Json,
        database: DatabaseVariant::Oracle,
        physical_format: PhysicalFormat::ClobJson,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::FunctionBased,
                supports_containment: false,
                supports_path_query: true,  // Single path only
                requires_expression: true,
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 10.0, // 10x slower baseline
        index_cost_multiplier: 0.2, // Func-based: ~5x speedup
        parse_overhead: ParseOverhead::PerAccess(0.15), // ~0.15ms parse per row
    }
}
```

#### MySQL 8.0 JSON

MySQL stores JSON in a binary format similar to PostgreSQL JSONB, with key-value
pairs sorted by key length for efficient lookup. MySQL 8.0.17+ supports
multi-valued indexes that index all values in a JSON array.

**Index support:**
- Multi-valued index: `CREATE INDEX ON t ((CAST(data->'$.tags' AS UNSIGNED ARRAY)))`
- Expression index: `CREATE INDEX ON t ((CAST(data->>'$.key' AS CHAR(N))))`
- No equivalent to PostgreSQL GIN for arbitrary containment queries

**Cost characteristics:**
- Key extraction (`->>`): O(log K) (binary search on sorted keys)
- `JSON_CONTAINS`: O(K) per row without index, O(log N) with multi-valued index
- Full document access: O(1) (binary format)

```rust
fn json_profile_mysql() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Json,
        database: DatabaseVariant::MySQL,
        physical_format: PhysicalFormat::BinaryJson,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::MultiValued,
                supports_containment: true, // Array containment only
                supports_path_query: false,
                requires_expression: true,
                setup_complexity: SetupComplexity::Moderate,
            },
            IndexCapability {
                index_type: StorageIndexType::BTree,
                supports_containment: false,
                supports_path_query: true,
                requires_expression: true,
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 2.0,
        index_cost_multiplier: 0.05, // MVI: ~20x speedup
        parse_overhead: ParseOverhead::None,
    }
}
```

#### SQL Server JSON

SQL Server has no native JSON data type. JSON is stored as `NVARCHAR(MAX)` and
queried using `JSON_VALUE()`, `JSON_QUERY()`, `OPENJSON()`. Every access parses
the text. Indexing requires adding a computed column with `JSON_VALUE` and then
creating a B-tree index on the computed column.

**Index support:**
- Computed column + B-tree: `ALTER TABLE ADD col AS JSON_VALUE(data, '$.path')`
- No containment index, no multi-valued index
- Full-text index on NVARCHAR: keyword search only

**Cost characteristics:**
- Path extraction (`JSON_VALUE`): O(D) per row (full text parse)
- Containment: Not natively supported; requires `OPENJSON` + `EXISTS`
- Full document access: O(1) (read NVARCHAR)
- With computed column index: O(log N) for indexed path only

```rust
fn json_profile_sqlserver() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Json,
        database: DatabaseVariant::SQLServer,
        physical_format: PhysicalFormat::TextJson,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::ComputedColumn,
                supports_containment: false,
                supports_path_query: true,
                requires_expression: true,
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 15.0,
        index_cost_multiplier: 0.1, // Computed col: ~10x speedup
        parse_overhead: ParseOverhead::PerAccess(0.12),
    }
}
```

### JSON Cost Model Summary

| Operation                | PostgreSQL | MySQL 8.0 | Oracle 21c | SQL Server |
|--------------------------|-----------|-----------|------------|------------|
| Key extraction (no idx)  | 0.001ms   | 0.001ms   | 0.15ms     | 0.12ms     |
| Containment (no idx)     | 0.002ms   | 0.003ms   | 0.20ms     | 0.25ms     |
| Containment (indexed)    | 0.00002ms | 0.0002ms  | 0.04ms     | N/A        |
| Path query (indexed)     | 0.0001ms  | 0.0001ms  | 0.001ms    | 0.001ms    |
| Parse overhead per row   | 0         | 0         | 0.15ms     | 0.12ms     |
| **Relative cost (scan)** | **1x**    | **2x**    | **100x**   | **80x**    |
| **Relative cost (idx)**  | **1x**    | **2.4x**  | **16x**    | **10x**    |

### XML Storage Adaptation

#### PostgreSQL XML

PostgreSQL has a native `xml` type that validates well-formedness. Internally it is
stored as text (with TOAST compression for large documents). XPath queries use the
`xpath()` function which parses the XML text on every invocation.

**Index support:**
- No dedicated XML index type
- Functional B-tree index on `xpath()` expressions for specific paths
- Full-text search via `to_tsvector(xml_data::text)` for keyword queries

**Cost characteristics:**
- XPath query: O(D) per row (full XML parse each time)
- Element extraction: O(D) per row
- With functional index on specific path: O(log N)
- Without index: O(N * D) -- worst case for large documents

```rust
fn xml_profile_postgresql() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Xml,
        database: DatabaseVariant::PostgreSQL,
        physical_format: PhysicalFormat::NativeXmlNoIndex,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::BTree,
                supports_containment: false,
                supports_path_query: true,
                requires_expression: true, // ON t ((xpath(...))::text)
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 10.0, // Relative to Oracle binary XML
        index_cost_multiplier: 0.1,
        parse_overhead: ParseOverhead::PerAccess(0.25), // XML parse per row
    }
}
```

#### Oracle XMLTYPE (Binary XML)

Oracle stores XMLTYPE with two options: CLOB (text) and binary XML. Binary XML
parses the document once at insert and stores a tokenized binary tree. Queries
traverse the binary representation directly. Oracle's XMLIndex creates a
structured index over element paths and values.

**Index support:**
- XMLIndex: indexes paths, values, and structure for arbitrary XQuery
- Function-based index on `XMLQuery()` for specific extractions
- Oracle Text index on XMLTYPE for full-text search

**Cost characteristics:**
- XPath/XQuery (binary storage): O(log D) per query (binary tree traversal)
- XPath/XQuery (CLOB storage): O(D) per query (text parse)
- With XMLIndex: O(log N) for arbitrary path queries
- Element extraction (binary): O(log K) where K = elements at that level

```rust
fn xml_profile_oracle() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Xml,
        database: DatabaseVariant::Oracle,
        physical_format: PhysicalFormat::BinaryXml,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::OracleXmlIndex,
                supports_containment: true,
                supports_path_query: true,
                requires_expression: false,
                setup_complexity: SetupComplexity::Moderate,
            },
            IndexCapability {
                index_type: StorageIndexType::FunctionBased,
                supports_containment: false,
                supports_path_query: true,
                requires_expression: true,
                setup_complexity: SetupComplexity::Moderate,
            },
        ],
        base_cost_multiplier: 1.0, // Baseline for XML
        index_cost_multiplier: 0.01,
        parse_overhead: ParseOverhead::OnceAtInsert,
    }
}
```

#### SQL Server XML

SQL Server has a native `xml` type with primary and secondary XML indexes.
A primary XML index shreds the XML document into a relational rowset (internal
table) at insert time. Secondary indexes (PATH, VALUE, PROPERTY) provide
optimized access patterns over the shredded data.

**Index support:**
- Primary XML index: creates internal relational representation
- Secondary PATH index: optimizes queries by path (`/root/element`)
- Secondary VALUE index: optimizes queries by value (`[text()="value"]`)
- Secondary PROPERTY index: optimizes property bag patterns

**Cost characteristics:**
- XQuery with primary index: O(log N) for most patterns
- XQuery with secondary PATH index: O(log N) for path-based queries
- XQuery without index: O(N * D) (full parse per row)
- `.exist()`, `.value()`, `.query()` all benefit from XML indexes

```rust
fn xml_profile_sqlserver() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Xml,
        database: DatabaseVariant::SQLServer,
        physical_format: PhysicalFormat::IndexedXml,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::XmlPrimary,
                supports_containment: true,
                supports_path_query: true,
                requires_expression: false,
                setup_complexity: SetupComplexity::Simple,
            },
            IndexCapability {
                index_type: StorageIndexType::XmlSecondary,
                supports_containment: true,
                supports_path_query: true,
                requires_expression: false,
                setup_complexity: SetupComplexity::Simple,
            },
        ],
        base_cost_multiplier: 1.5, // Slightly slower than Oracle binary XML
        index_cost_multiplier: 0.02,
        parse_overhead: ParseOverhead::OnceAtInsert, // Shredded at insert
    }
}
```

### XML Cost Model Summary

| Operation                 | PostgreSQL | Oracle (binary) | SQL Server | Oracle (CLOB) |
|---------------------------|-----------|-----------------|------------|---------------|
| XPath extract (no idx)    | 0.25ms    | 0.02ms          | 0.03ms     | 0.30ms        |
| XPath search (no idx)     | 0.25ms    | 0.02ms          | 0.03ms     | 0.30ms        |
| XPath search (indexed)    | 0.001ms   | 0.0002ms        | 0.0005ms   | 0.001ms       |
| Parse overhead per row    | 0.25ms    | 0               | 0          | 0.30ms        |
| **Relative cost (scan)**  | **12x**   | **1x**          | **1.5x**   | **15x**       |
| **Relative cost (idx)**   | **5x**    | **1x**          | **2.5x**   | **5x**        |

Note the reversal from JSON: PostgreSQL is the *slowest* for XML operations because
it lacks dedicated XML indexes. Oracle with binary XML storage is fastest. The
optimizer must not assume PostgreSQL is always the performance leader.

### Spatial Type Storage Adaptation

#### PostGIS (PostgreSQL)

PostGIS extends PostgreSQL with GEOMETRY and GEOGRAPHY types. Data is stored as
WKB (Well-Known Binary) in the heap. GiST indexes provide R-tree-like spatial
indexing with bounding box filtering. SP-GiST provides space-partitioned indexes
for point data.

**Index support:**
- GiST index: bounding box queries, nearest-neighbor, containment
- SP-GiST index: point-only queries, quad-tree partitioning
- BRIN index: for spatially sorted data (bulk loading)

**Cost characteristics:**
- Distance query with GiST: O(log N) + recheck
- Containment (ST_Contains) with GiST: O(log N) + recheck
- Nearest-neighbor (ORDER BY ... <->): O(log N) via index
- Without index: O(N) with computational geometry per row

```rust
fn spatial_profile_postgis() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Spatial,
        database: DatabaseVariant::PostgreSQL,
        physical_format: PhysicalFormat::PostGisNative,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::Gist,
                supports_containment: true,
                supports_path_query: false,
                requires_expression: false,
                setup_complexity: SetupComplexity::Simple,
            },
        ],
        base_cost_multiplier: 1.0, // Baseline for spatial
        index_cost_multiplier: 0.01,
        parse_overhead: ParseOverhead::None, // WKB is binary
    }
}
```

#### Oracle Spatial (SDO_GEOMETRY)

Oracle stores spatial data as `SDO_GEOMETRY`, a structured object type with
coordinate arrays and metadata. Spatial indexing uses R-tree indexes that must
be registered in `USER_SDO_GEOM_METADATA` before creation. Oracle Spatial
supports 2D, 3D, and LRS (Linear Referencing System) geometries.

**Index support:**
- R-tree spatial index: requires metadata registration in `USER_SDO_GEOM_METADATA`
- Supports `SDO_RELATE`, `SDO_WITHIN_DISTANCE`, `SDO_NN` (nearest neighbor)
- Tessellation-based indexing for complex geometries

**Cost characteristics:**
- Distance query with R-tree: O(log N) + filter step
- Containment with R-tree: O(log N) + precise geometry check
- Nearest-neighbor (`SDO_NN`): O(log N)
- Setup cost: Higher than PostGIS (metadata registration, manual tuning)

```rust
fn spatial_profile_oracle() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Spatial,
        database: DatabaseVariant::Oracle,
        physical_format: PhysicalFormat::OracleSdo,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::SpatialRTree,
                supports_containment: true,
                supports_path_query: false,
                requires_expression: false,
                setup_complexity: SetupComplexity::Complex, // Metadata registration
            },
        ],
        base_cost_multiplier: 2.5,  // R-tree slightly slower than GiST
        index_cost_multiplier: 0.015,
        parse_overhead: ParseOverhead::None,
    }
}
```

#### SQL Server Spatial

SQL Server provides `GEOMETRY` (planar) and `GEOGRAPHY` (geodetic) types.
Spatial indexes use a multi-level grid decomposition (4 levels by default).
Grid density settings (LOW, MEDIUM, HIGH) affect index selectivity.

**Index support:**
- Grid-based spatial index with configurable density per level
- Supports `STDistance`, `STContains`, `STIntersects`
- Tessellation cells for bounding-box filtering

**Cost characteristics:**
- Distance query with grid index: O(log N) + grid cell enumeration
- Containment with grid index: O(log N) + geometry recheck
- Grid overhead: More cells checked than R-tree for irregular shapes
- Index size: Larger than GiST/R-tree for same data (grid decomposition)

```rust
fn spatial_profile_sqlserver() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Spatial,
        database: DatabaseVariant::SQLServer,
        physical_format: PhysicalFormat::SqlServerSpatial,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::SpatialGrid,
                supports_containment: true,
                supports_path_query: false,
                requires_expression: false,
                setup_complexity: SetupComplexity::Moderate, // Grid density config
            },
        ],
        base_cost_multiplier: 4.0,  // Grid decomposition overhead
        index_cost_multiplier: 0.02,
        parse_overhead: ParseOverhead::None,
    }
}
```

#### MySQL 8.0 Spatial

MySQL 8.0 supports `GEOMETRY` types with InnoDB spatial indexes (R-tree).
Earlier versions required MyISAM for spatial indexes. The spatial function
library is smaller than PostGIS but covers standard operations.

**Cost characteristics:**
- Distance query with R-tree: O(log N) + recheck
- Limited function library compared to PostGIS
- No GEOGRAPHY type (geodetic calculations must be manual)

```rust
fn spatial_profile_mysql() -> TypeStorageProfile {
    TypeStorageProfile {
        logical_type: LogicalType::Spatial,
        database: DatabaseVariant::MySQL,
        physical_format: PhysicalFormat::MySqlSpatial,
        index_capabilities: vec![
            IndexCapability {
                index_type: StorageIndexType::Gist, // InnoDB R-tree
                supports_containment: true,
                supports_path_query: false,
                requires_expression: false,
                setup_complexity: SetupComplexity::Simple,
            },
        ],
        base_cost_multiplier: 3.0,
        index_cost_multiplier: 0.02,
        parse_overhead: ParseOverhead::None,
    }
}
```

### Spatial Cost Model Summary

| Operation                  | PostGIS   | Oracle Spatial | SQL Server | MySQL 8.0 |
|----------------------------|-----------|----------------|------------|-----------|
| Distance query (indexed)   | 2ms       | 5ms            | 8ms        | 6ms       |
| Containment (indexed)      | 1.5ms     | 4ms            | 7ms        | 5ms       |
| Nearest-neighbor (indexed) | 2ms       | 5ms            | 12ms       | N/A       |
| Distance query (no idx)    | 2,800ms   | 3,500ms        | 4,200ms    | 3,800ms   |
| **Relative cost (idx)**    | **1x**    | **2.5x**       | **4x**     | **3x**    |
| **Relative cost (scan)**   | **1x**    | **1.25x**      | **1.5x**   | **1.35x** |

### Unified Cost Estimation

```rust
impl CostEstimator {
    /// Estimate cost of a type-specific operation.
    /// Returns cost in milliseconds per row.
    pub fn estimate_typed_operation(
        &self,
        profile: &TypeStorageProfile,
        operation: TypedOperation,
        has_index: bool,
        row_count: u64,
    ) -> f64 {
        let per_row_cost = match operation {
            TypedOperation::JsonContainment => 0.002,
            TypedOperation::JsonPathExtract => 0.001,
            TypedOperation::XmlXpathSearch => 0.02,
            TypedOperation::XmlElementExtract => 0.015,
            TypedOperation::SpatialDistance => 0.05,
            TypedOperation::SpatialContainment => 0.04,
            TypedOperation::SpatialNearestNeighbor => 0.06,
        };

        let parse_cost = match profile.parse_overhead {
            ParseOverhead::None => 0.0,
            ParseOverhead::PerAccess(cost) => cost,
            ParseOverhead::OnceAtInsert => 0.0,
        };

        let adjusted_per_row = (per_row_cost + parse_cost)
            * profile.base_cost_multiplier;

        if has_index {
            // Indexed access: logarithmic scan + recheck fraction
            let index_rows = (row_count as f64).log2() * 2.0;
            index_rows * adjusted_per_row * profile.index_cost_multiplier
        } else {
            // Full scan: every row
            row_count as f64 * adjusted_per_row
        }
    }
}
```

### Optimization Strategy Selection

```rust
impl Optimizer {
    /// Select optimization strategy based on storage profile.
    pub fn adapt_strategy(
        &self,
        profile: &TypeStorageProfile,
        query: &TypedQuery,
    ) -> OptimizationStrategy {
        match (profile.logical_type, profile.database) {
            // JSON strategies
            (LogicalType::Json, DatabaseVariant::PostgreSQL) => {
                // Rewrite equality predicates to containment (@>)
                // Recommend GIN index on entire column
                OptimizationStrategy::JsonContainmentRewrite {
                    operator: "@>".into(),
                    index_type: "GIN".into(),
                    index_expression: None,
                }
            }
            (LogicalType::Json, DatabaseVariant::Oracle) => {
                // Minimize JSON_VALUE calls in WHERE clause
                // Recommend function-based index per queried path
                // Cache extracted values in computed columns
                OptimizationStrategy::JsonFunctionIndex {
                    function: "JSON_VALUE".into(),
                    paths: query.accessed_json_paths(),
                    warn_clob_overhead: true,
                }
            }
            (LogicalType::Json, DatabaseVariant::MySQL) => {
                // Use multi-valued index for array containment
                // Use expression index for scalar path queries
                OptimizationStrategy::JsonExpressionIndex {
                    containment_func: "JSON_CONTAINS".into(),
                    path_syntax: "->>".into(),
                }
            }
            (LogicalType::Json, DatabaseVariant::SQLServer) => {
                // Add computed columns for frequently queried paths
                // Index computed columns with B-tree
                OptimizationStrategy::JsonComputedColumn {
                    function: "JSON_VALUE".into(),
                    paths: query.accessed_json_paths(),
                }
            }

            // XML strategies
            (LogicalType::Xml, DatabaseVariant::PostgreSQL) => {
                // Avoid XML in hot paths if possible
                // Use functional index on specific xpath()
                // Consider extracting to relational columns
                OptimizationStrategy::XmlFunctionalIndex {
                    function: "xpath".into(),
                    warn_no_xml_index: true,
                }
            }
            (LogicalType::Xml, DatabaseVariant::Oracle) => {
                // Use binary XML storage option
                // Create XMLIndex for structural queries
                // Leverage XQuery optimization
                OptimizationStrategy::XmlNativeIndex {
                    index_type: "XMLIndex".into(),
                    prefer_binary_storage: true,
                }
            }
            (LogicalType::Xml, DatabaseVariant::SQLServer) => {
                // Create primary XML index
                // Add secondary indexes matching query patterns
                OptimizationStrategy::XmlSecondaryIndexes {
                    primary: true,
                    secondary_types: query.xml_index_hints(),
                }
            }

            // Spatial strategies
            (LogicalType::Spatial, DatabaseVariant::PostgreSQL) => {
                // GiST index, use ST_ functions
                // Prefer geography type for geodetic queries
                OptimizationStrategy::SpatialGist {
                    index_type: "GiST".into(),
                    prefer_geography: query.is_geodetic(),
                }
            }
            (LogicalType::Spatial, DatabaseVariant::Oracle) => {
                // Ensure SDO_GEOM_METADATA is registered
                // Create R-tree spatial index
                OptimizationStrategy::SpatialRTree {
                    requires_metadata: true,
                    srid: query.srid(),
                }
            }
            (LogicalType::Spatial, DatabaseVariant::SQLServer) => {
                // Configure grid densities based on data distribution
                OptimizationStrategy::SpatialGrid {
                    grid_levels: 4,
                    default_density: "MEDIUM".into(),
                }
            }
            (LogicalType::Spatial, DatabaseVariant::MySQL) => {
                // InnoDB R-tree index
                // Warn about limited function library
                OptimizationStrategy::SpatialBasic {
                    index_type: "SPATIAL".into(),
                    warn_limited_functions: true,
                }
            }

            _ => OptimizationStrategy::NoAdaptation,
        }
    }
}
```

### Integration Points

**1. Cost Model (ra-core):**
Storage profiles feed into the cost model via `estimate_typed_operation()`. Every
plan node involving a typed column consults the storage profile to adjust its
cost estimate.

**2. Index Advisor (RFC 0021):**
The index advisor uses `IndexCapability` from storage profiles to recommend
database-appropriate indexes. It avoids suggesting GIN indexes for Oracle or
XMLIndex for PostgreSQL.

**3. Query Rewriter:**
The rewriter translates between database-specific syntaxes:
- PostgreSQL `@>` to Oracle `JSON_EXISTS()`
- Oracle `XMLExists()` to SQL Server `.exist()`
- PostGIS `ST_DWithin()` to Oracle `SDO_WITHIN_DISTANCE()`

**4. Dialect Translation (RFC 0008):**
Storage profiles extend dialect translation with type-aware function mapping
and index DDL generation.

**5. Type Support (RFC 0055):**
RFC 0055 defines the logical type system. This RFC adds storage-level detail
per database for each logical type.

**6. PostgreSQL Optimizations (RFC 0056):**
RFC 0056 provides deep PostgreSQL-specific rules. This RFC provides the
cross-database comparison framework that contextualizes those rules.

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageAdaptationError {
    #[error(
        "no storage profile for {logical_type:?} on {database:?}"
    )]
    UnknownProfile {
        logical_type: LogicalType,
        database: DatabaseVariant,
    },

    #[error(
        "index type {index_type:?} not supported for \
         {logical_type:?} on {database:?}"
    )]
    UnsupportedIndex {
        index_type: StorageIndexType,
        logical_type: LogicalType,
        database: DatabaseVariant,
    },

    #[error(
        "cost multiplier out of range: {value} \
         (expected 0.01..1000.0)"
    )]
    InvalidCostMultiplier { value: f64 },
}
```

## Drawbacks

**Maintenance burden:**
- Storage characteristics change with database releases (Oracle 21c introduced
  native binary JSON; MySQL 8.0 added InnoDB spatial; SQL Server 2022 improved
  JSON support). Each release requires profile updates.

**Estimation uncertainty:**
- Cost multipliers are derived from benchmarks on specific hardware and data
  distributions. Actual multipliers vary by 2-3x depending on hardware, data
  size, document complexity, and concurrent load. Presenting single numbers
  risks false precision.

**Testing complexity:**
- Validating storage profiles requires running benchmarks on each database
  engine. Automated tests can verify profile construction and cost calculation
  logic, but not that the multipliers reflect reality.

**Incomplete coverage:**
- This RFC covers JSON, XML, and spatial types across four databases. Other
  types (arrays, range types, full-text search, hierarchical types) use the
  same framework but are not yet profiled.

## Rationale and alternatives

### Why storage-aware cost models?

The alternative is treating all implementations of a logical type as equivalent.
This produces plans that are correct but poorly optimized. A JSON containment
predicate costs 0.002ms/row on PostgreSQL JSONB and 0.20ms/row on Oracle CLOB --
a 100x difference. An optimizer unaware of this difference will underestimate
Oracle query costs and may choose a plan that scans the JSON column when a
join-first strategy would be faster.

### Why per-database profiles instead of runtime calibration?

Runtime calibration (executing test queries to measure actual cost) provides more
accurate numbers but requires a live database connection, adds latency to
optimization, and cannot be used during offline analysis or migration planning.
Static profiles provide reasonable estimates immediately. RFC 0026 (Adaptive Cost
Calibration) can refine these estimates with runtime feedback.

### Why not a single "best database" recommendation?

Users are often constrained to a specific database by organizational policy,
licensing, existing infrastructure, or application requirements. Ra must optimize
well on every supported database, not just recommend switching to PostgreSQL.

### Alternative: user-provided multipliers

Allowing users to override cost multipliers adds flexibility but shifts the
burden to users who may not have benchmark data. The default profiles should be
accurate enough for plan selection; users can override via configuration when
they have measured data.

### Impact of not doing this

- Cost model produces identical estimates for PostgreSQL JSONB and Oracle CLOB JSON
- Index advisor recommends GIN indexes for Oracle (which does not support them)
- Migration planning has no performance impact estimates
- Optimizer may select plans that perform 10-100x worse than optimal

## Prior art

### Apache Calcite

Calcite's adapter framework connects to multiple databases through a common
relational algebra. Each adapter provides cost estimates based on the backend's
capabilities. However, Calcite adapters operate at the table/query level, not
at the type-storage level. Calcite does not differentiate between JSONB and
CLOB JSON storage for the same logical operation.

### Presto/Trino Connector API

Presto connectors push predicates and projections down to the data source.
Each connector knows what operations the backend supports efficiently. This
is analogous to our storage profiles: the connector for PostgreSQL knows
JSONB containment is cheap, while the Oracle connector knows JSON_EXISTS
is expensive. However, Presto connectors are execution-level, not
optimization-level -- they decide what to push down, not how to cost
alternatives.

### AWS Schema Conversion Tool

AWS SCT maps types between databases during migration (e.g., Oracle CLOB to
PostgreSQL TEXT). It provides compatibility warnings but not performance
estimates. Ra's storage profiles extend this concept by attaching
performance characteristics to each type mapping.

### PostgreSQL Foreign Data Wrappers (FDW)

FDW allows PostgreSQL to query external databases. The `postgres_fdw` wrapper
pushes operations to the remote server. However, the cost model for remote
operations is simplistic (configurable multiplier per table, not per type).
Our approach provides type-level cost granularity.

## Unresolved questions

1. **Version-specific profiles**: Oracle 19c CLOB JSON vs Oracle 21c binary
   JSON type have different performance characteristics. Should profiles
   be versioned per database release? Initial approach: model the most
   common production version, add version overrides as needed.

2. **Cost multiplier calibration**: How to validate that multipliers reflect
   real-world performance? Proposal: publish benchmark suite that users can
   run on their hardware, with results feeding back into profile defaults.

3. **Fallback when storage info is unavailable**: If the optimizer cannot
   determine the target database or storage format, should it assume
   worst-case (conservative) or average-case? Proposal: default to
   PostgreSQL-like cost model (most commonly used in development) with
   a warning that estimates may be inaccurate.

4. **Hybrid storage within one database**: Oracle allows XMLTYPE to be stored
   as CLOB or binary XML on the same database, even in different tables.
   Should profiles be per-table rather than per-database? Proposal:
   per-database default with per-column overrides.

5. **User-tunable multipliers**: Should users be able to adjust cost
   multipliers via configuration? Proposal: yes, as optional overrides,
   but defaults should be accurate enough for most workloads.

## Future possibilities

### Learned cost models

Replace static multipliers with models trained on actual query execution
data from each database. RFC 0026 (Adaptive Cost Calibration) provides
the feedback mechanism; storage profiles provide the structure.

### Automatic query rewriting for migration

Given a set of PostgreSQL queries with JSONB operations, generate equivalent
Oracle queries with appropriate JSON_VALUE/JSON_EXISTS calls and recommended
function-based indexes.

### Storage format recommendations

Analyze workload patterns and recommend optimal storage format per column:
"Oracle XMLTYPE binary storage would be 15x faster than CLOB for your
XPath-heavy workload."

### Cross-database federated optimization

When a query spans multiple databases (e.g., PostgreSQL JSON data joined with
Oracle spatial data), use storage profiles to decide which operations to execute
where, minimizing data transfer and leveraging each database's strengths.

### Continuous calibration

Monitor actual query execution times and update cost multipliers automatically.
Detect when a database upgrade changes storage characteristics (e.g., Oracle
migrating from CLOB to binary JSON) and adjust profiles accordingly.
