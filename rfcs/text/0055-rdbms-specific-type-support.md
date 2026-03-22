# RFC 0055: RDBMS-Specific Type Support and Optimizations

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Add comprehensive support for database-specific types (PostgreSQL JSONB/XML/HSTORE, Oracle CLOB/XMLTYPE, SQL Server HIERARCHYID, MySQL JSON) with corresponding optimization rules. This enables Ra to optimize queries using these advanced types and provide database-specific indexing recommendations.

## Motivation

Modern databases support advanced data types beyond traditional SQL (INTEGER, VARCHAR, DATE). These types power critical features:

- **Document stores**: JSONB in PostgreSQL, JSON in MySQL
- **XML processing**: PostgreSQL XML, Oracle XMLTYPE
- **Spatial data**: PostGIS GEOMETRY, Oracle SDO_GEOMETRY
- **Large objects**: PostgreSQL TOAST, Oracle CLOB/BLOB
- **Hierarchies**: SQL Server HIERARCHYID

**Problems:**

1. **Ra currently treats these as opaque strings**: No type-aware optimization
2. **Index selection misses specialized indexes**: GIN for JSONB, GiST for spatial
3. **Cost model ignores type-specific overhead**: TOAST reads, XML parsing, JSON extraction
4. **Cross-database migration is hard**: Different databases store "JSON" differently

**This RFC enables:**

- Parsing and representing database-specific types in Ra's type system
- Type-aware optimization rules (JSONB containment → GIN index)
- Type-specific cost model adjustments (TOAST overhead, XML parsing cost)
- Cross-database type mapping (Oracle JSON → PostgreSQL JSONB)

## Guide-level explanation

### PostgreSQL JSONB Example

**Query:**

```sql
SELECT user_id, data->>'name' AS name
FROM users
WHERE data @> '{"status": "active", "verified": true}';
```

**Without type support:**

- Ra treats `data` as generic column
- Misses GIN index on `data` column
- No optimization for `@>` operator (containment)
- Cost model ignores JSON extraction overhead

**With type support:**

```rust
// Ra recognizes JSONB type
let data_col = Column {
    name: "data",
    type: Type::PostgreSQL(PostgreSQLType::Jsonb),
};

// Detects JSONB containment operator
let predicate = BinOp {
    op: Op::JsonContains,  // @>
    left: col("data"),
    right: jsonb_literal(r#"{"status": "active", "verified": true}"#),
};

// Optimizer suggests GIN index
let recommendation = IndexRecommendation {
    table: "users",
    columns: vec!["data"],
    index_type: IndexType::Gin,
    rationale: "JSONB containment query benefits from GIN index",
};
```

**Optimizations applied:**

1. **GIN index selection**: Use `CREATE INDEX idx_users_data_gin ON users USING GIN (data)`
2. **Predicate transformation**: Convert `data->>'key' = 'value'` to `data @> '{"key": "value"}'` (indexable)
3. **Cost adjustment**: Add JSON extraction overhead to cost model

### Oracle XMLTYPE Example

**Query:**

```sql
SELECT doc_id
FROM documents
WHERE XMLExists('/document/author[text()="Smith"]' PASSING xmldoc);
```

**With type support:**

```rust
// Recognize Oracle XMLTYPE
let xmldoc_col = Column {
    name: "xmldoc",
    type: Type::Oracle(OracleType::XmlType),
};

// Suggest XMLIndex
let recommendation = IndexRecommendation {
    table: "documents",
    columns: vec!["xmldoc"],
    index_type: IndexType::OracleXmlIndex,
    rationale: "XMLExists query benefits from XMLIndex",
};
```

## Reference-level explanation

### Implementation Details

**Type System Extension:**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    // Standard SQL types
    Integer,
    BigInt,
    Varchar(Option<usize>),
    Text,
    Boolean,
    Date,
    Timestamp,
    Numeric(Option<u8>, Option<u8>),

    // Database-specific types
    PostgreSQL(PostgreSQLType),
    Oracle(OracleType),
    SQLServer(SQLServerType),
    MySQL(MySQLType),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PostgreSQLType {
    Jsonb,
    Json,
    Xml,
    Hstore,
    Array(Box<Type>),
    Range(RangeType),  // int4range, tsrange, etc.
    Uuid,
    Inet,
    Cidr,
    MacAddr,
    Citext,
    // PostGIS types
    Geometry,
    Geography,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OracleType {
    Clob,
    Blob,
    NClob,
    XmlType,
    Json,  // Stored as CLOB with IS JSON constraint
    ObjectType(String),  // User-defined object types
    VArray(Box<Type>),
    NestedTable(Box<Type>),
    // Oracle Spatial
    SdoGeometry,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SQLServerType {
    Xml,
    HierarchyId,
    Geometry,
    Geography,
    NVarcharMax,
    VarcharMax,
    VarbinaryMax,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MySQLType {
    Json,
    Text(TextSize),      // TINYTEXT, TEXT, MEDIUMTEXT, LONGTEXT
    Blob(BlobSize),      // TINYBLOB, BLOB, MEDIUMBLOB, LONGBLOB
    Enum(Vec<String>),
    Set(Vec<String>),
    Geometry,
    Point,
    LineString,
    Polygon,
}
```

**Type-Specific Operators:**

```rust
pub enum Op {
    // Standard SQL operators
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or, Not,
    Like, In,

    // JSONB operators (PostgreSQL)
    JsonContains,           // @>
    JsonContainedBy,        // <@
    JsonExists,             // ?
    JsonExistsAny,          // ?|
    JsonExistsAll,          // ?&
    JsonConcat,             // ||
    JsonDelete,             // -
    JsonPathQuery,          // @?
    JsonPathMatch,          // @@

    // XML operators
    XmlExists,              // XMLExists()
    XmlQuery,               // XMLQuery()
    XmlTable,               // XMLTable()

    // Array operators (PostgreSQL)
    ArrayContains,          // @>
    ArrayOverlap,           // &&
    ArrayConcat,            // ||

    // Spatial operators
    SpatialIntersects,      // ST_Intersects
    SpatialWithin,          // ST_Within
    SpatialContains,        // ST_Contains
}
```

**Optimization Rules:**

**Rule 1: JSONB Predicate Transformation**

Transform non-indexable JSON operations into indexable containment:

```rust
// Input: data->>'status' = 'active'
// Output: data @> '{"status": "active"}'

pub fn jsonb_predicate_transform(expr: &Expr) -> Option<Expr> {
    match expr {
        Expr::BinOp {
            op: Op::Eq,
            left: box Expr::JsonExtractText { object, path },
            right: box Expr::Const(Const::String(value)),
        } => {
            // Transform to containment
            Some(Expr::BinOp {
                op: Op::JsonContains,
                left: object.clone(),
                right: Box::new(Expr::Const(Const::Jsonb(
                    serde_json::json!({ path: value })
                ))),
            })
        }
        _ => None,
    }
}
```

**Rule 2: GIN Index Suggestion**

```rust
impl IndexAdvisor {
    pub fn suggest_gin_index(
        &self,
        table: &str,
        column: &str,
        column_type: &PostgreSQLType,
        predicates: &[Expr],
    ) -> Option<IndexRecommendation> {
        match column_type {
            PostgreSQLType::Jsonb => {
                // Check if any predicate uses @>, @?, ?, etc.
                let uses_containment = predicates.iter().any(|p| {
                    matches!(p, Expr::BinOp {
                        op: Op::JsonContains | Op::JsonContainedBy | Op::JsonExists,
                        ..
                    })
                });

                if uses_containment {
                    Some(IndexRecommendation {
                        table: table.to_string(),
                        columns: vec![column.to_string()],
                        index_type: IndexType::Gin,
                        rationale: format!(
                            "JSONB containment queries benefit from GIN index on {}.{}",
                            table, column
                        ),
                        estimated_speedup: 10.0,  // 10x faster with GIN
                    })
                } else {
                    None
                }
            }
            PostgreSQLType::Array(_) => {
                // Similar logic for arrays
                // ...
            }
            _ => None,
        }
    }
}
```

**Rule 3: TOAST-Aware Cost Model**

```rust
impl CostModel {
    pub fn estimate_column_read_cost(&self, column: &Column, stats: &Statistics) -> Cost {
        let base_cost = self.base_io_cost;

        match &column.col_type {
            Type::PostgreSQL(PostgreSQLType::Text) |
            Type::PostgreSQL(PostgreSQLType::Jsonb) |
            Type::Oracle(OracleType::Clob) => {
                // Check if column is TOASTed (PostgreSQL)
                let avg_size = stats.avg_column_size(&column.name);
                if avg_size > 2048 {  // TOAST threshold
                    // TOASTed columns require additional I/O
                    base_cost * 2.0  // 2x cost for out-of-line storage
                } else {
                    base_cost
                }
            }
            Type::Oracle(OracleType::XmlType) => {
                // XML parsing overhead
                base_cost * 1.5
            }
            _ => base_cost,
        }
    }
}
```

**Rule 4: Cross-Database Type Mapping**

```rust
pub fn map_type_across_databases(
    source_db: Database,
    target_db: Database,
    source_type: &Type,
) -> Result<Type, TypeMappingError> {
    match (source_db, target_db, source_type) {
        // Oracle JSON → PostgreSQL JSONB
        (Database::Oracle, Database::PostgreSQL, Type::Oracle(OracleType::Json)) => {
            Ok(Type::PostgreSQL(PostgreSQLType::Jsonb))
        }

        // MySQL JSON → PostgreSQL JSONB
        (Database::MySQL, Database::PostgreSQL, Type::MySQL(MySQLType::Json)) => {
            Ok(Type::PostgreSQL(PostgreSQLType::Jsonb))
        }

        // PostgreSQL JSONB → Oracle JSON (with warning)
        (Database::PostgreSQL, Database::Oracle, Type::PostgreSQL(PostgreSQLType::Jsonb)) => {
            // Oracle stores JSON as CLOB, less efficient
            eprintln!("Warning: PostgreSQL JSONB → Oracle JSON (CLOB-based, slower)");
            Ok(Type::Oracle(OracleType::Json))
        }

        // PostgreSQL Array → Oracle VARRAY
        (Database::PostgreSQL, Database::Oracle, Type::PostgreSQL(PostgreSQLType::Array(elem_type))) => {
            let mapped_elem = map_type_across_databases(source_db, target_db, elem_type)?;
            Ok(Type::Oracle(OracleType::VArray(Box::new(mapped_elem))))
        }

        _ => Err(TypeMappingError::UnsupportedConversion {
            source: source_type.clone(),
            source_db,
            target_db,
        }),
    }
}
```

### Integration Points

**1. Parser Integration:**

Extend SQL parser to recognize type-specific operators:

```rust
// PostgreSQL JSONB operators
parser.register_operator("@>", Op::JsonContains, Associativity::Left, 7);
parser.register_operator("<@", Op::JsonContainedBy, Associativity::Left, 7);
parser.register_operator("?", Op::JsonExists, Associativity::Left, 7);

// Spatial operators
parser.register_function("ST_Intersects", FnKind::Spatial(SpatialFn::Intersects));
```

**2. Statistics Integration:**

Collect type-specific statistics:

```rust
pub struct TypeSpecificStats {
    // JSONB statistics
    pub jsonb_key_frequency: HashMap<String, f64>,  // Most common keys
    pub jsonb_avg_depth: f64,                        // Average nesting depth

    // XML statistics
    pub xml_avg_size: usize,
    pub xml_common_paths: Vec<String>,

    // Array statistics
    pub array_avg_length: f64,
    pub array_element_type: Type,
}
```

**3. Index Advisor Integration (RFC 0021):**

```rust
impl IndexAdvisor {
    pub fn recommend_for_type(&self, column: &Column, workload: &Workload) -> Vec<IndexRecommendation> {
        match &column.col_type {
            Type::PostgreSQL(PostgreSQLType::Jsonb) => self.recommend_jsonb_indexes(column, workload),
            Type::PostgreSQL(PostgreSQLType::Array(_)) => self.recommend_gin_indexes(column, workload),
            Type::PostgreSQL(PostgreSQLType::Geometry) => self.recommend_gist_indexes(column, workload),
            _ => vec![],
        }
    }
}
```

**4. Query Parser (RFC 0053):**

Stored procedures may use type-specific operators, need to parse and optimize them.

### Error Handling

```rust
#[derive(Debug, Error)]
pub enum TypeError {
    #[error("Unsupported type for database {database}: {type_name}")]
    UnsupportedType { database: Database, type_name: String },

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: Type, actual: Type },

    #[error("Cannot convert {source_type} from {source_db} to {target_db}")]
    UnsupportedConversion { source_type: Type, source_db: Database, target_db: Database },

    #[error("Operator {op} not supported for type {type_name}")]
    UnsupportedOperator { op: Op, type_name: String },
}
```

### Performance Considerations

**Parsing Overhead:**

- Type-specific operators require additional parser rules
- Impact: Minimal (<1% parsing time increase)

**Cost Model Complexity:**

- Type-specific cost adjustments require more computation
- TOAST detection requires statistics lookup
- Impact: Acceptable, cost estimation is not on critical path

**Memory:**

- Extended type system increases Type enum size
- Impact: Minimal, few thousand Type values per query

## Drawbacks

**Complexity:**

- Adds 100+ new type variants across 4 databases
- Each type requires operator definitions, cost model adjustments, index rules
- Maintenance burden as databases add new types

**Database-Specific Code:**

- Much code is database-specific, not reusable
- Risk of divergence between database implementations
- Testing requires access to multiple databases

**Incomplete Coverage:**

- Cannot support every database-specific type immediately
- Some types are poorly documented (e.g., Oracle spatial)
- User-defined types are out of scope

**Optimization Uncertainty:**

- Type-specific optimizations may not always help
- Cost model adjustments are estimates (actual performance varies)
- Risk of over-optimizing for one database, degrading portability

## Rationale and alternatives

### Why This Design?

**Enum-Based Type System:**

- Clear, type-safe representation of database-specific types
- Easy to pattern match and transform
- Compile-time checking prevents type errors

**Database-Specific Namespaces:**

- `Type::PostgreSQL(...)`, `Type::Oracle(...)` clearly separate dialects
- Avoids name collisions (PostgreSQL JSON vs MySQL JSON)
- Makes cross-database mapping explicit

**Type-Aware Optimization:**

- Leverage specialized indexes (GIN, GiST, XMLIndex)
- Accurate cost models (TOAST overhead)
- Better than treating everything as TEXT

### Alternative Approaches

**1. Generic "JSON" Type:**

- Single `Type::Json` for all databases
- **Rejected**: Hides important differences (PostgreSQL JSONB binary vs Oracle JSON CLOB)

**2. Opaque User-Defined Types:**

- Single `Type::UserDefined(String)` for all database-specific types
- **Rejected**: Cannot optimize, no type safety

**3. Plugin System:**

- Each database provides a plugin with type definitions
- **Rejected**: Too complex for initial version, adds indirection

**4. Ignore Database-Specific Types:**

- Only support standard SQL types
- **Rejected**: Misses major use cases (JSON, XML, spatial)

### Impact of Not Doing This

**Without database-specific type support:**

- Ra cannot optimize JSONB/XML/spatial queries
- Index recommendations miss specialized indexes (GIN, GiST)
- Cost model underestimates TOAST overhead
- Cross-database migration is harder (no type mapping)

**Workaround:**

- Users manually specify indexes
- Treat database-specific types as TEXT (inefficient)
- Use database-specific tools for optimization

## Prior art

### Academic Research

**Type Systems for Databases:**

- [Comprehensive Data Type Systems](https://dl.acm.org/doi/10.1145/320434.320440) - Early work on database type systems
- [Optimization of Object-Oriented Queries](https://dl.acm.org/doi/10.1145/253262.253302) - Optimizing complex types

### Industry Solutions

**PostgreSQL:**

- **Rich type system**: 40+ built-in types, extensible via CREATE TYPE
- **Type-specific indexes**: GIN (JSONB, arrays), GiST (spatial, ranges), SP-GiST (trees)
- **Type-aware operators**: 200+ operators, overloaded by type
- **TOAST**: Automatic out-of-line storage for large values

**Oracle:**

- **XMLTYPE**: Native XML storage with XMLIndex
- **Object types**: User-defined types with methods
- **Collections**: VARRAY, NESTED TABLE
- **LOB types**: CLOB, BLOB, NCLOB with chunk-based storage

**SQL Server:**

- **XML type**: Native XML with XML indexes (primary, secondary)
- **HIERARCHYID**: Tree structures with hierarchical queries
- **Spatial types**: GEOMETRY, GEOGRAPHY with spatial indexes
- **MAX types**: VARCHAR(MAX), VARBINARY(MAX)

**MySQL:**

- **JSON type**: Native JSON (MySQL 5.7+) with multi-valued indexes
- **ENUM/SET**: Efficient storage for fixed value sets
- **Spatial types**: GEOMETRY, POINT, LINESTRING, POLYGON
- **TEXT/BLOB sizes**: TINYTEXT, TEXT, MEDIUMTEXT, LONGTEXT

**Apache Calcite:**

- **Extensible type system**: `RelDataType` interface
- **Type factories**: Per-database type factories
- **Operator table**: Per-database operator overloading
- **Limited type-specific optimization**: Treats most types generically

**What We Can Learn:**

- Type system must be extensible (new types added frequently)
- Operator overloading is essential (same operator, different types)
- Specialized indexes are key to performance (GIN, GiST, XMLIndex)
- Cross-database type mapping is valuable (migration tools)
- TOAST/LOB handling significantly affects cost model

## Unresolved questions

**Design Questions:**

1. Should Ra support user-defined types (CREATE TYPE)? (Initial: No, future work)
2. How to handle type coercion between database-specific types? (e.g., PostgreSQL JSON → JSONB)
3. Should type-specific optimizations be opt-in or automatic?

**Implementation Questions:**

1. Which types should be implemented first? (Recommendation: PostgreSQL JSONB, Oracle CLOB)
2. How to collect type-specific statistics? (Extend Statistics struct or separate?)
3. Should type mapping be bidirectional (Oracle → PostgreSQL and PostgreSQL → Oracle)?

**Integration Questions:**

1. How to expose type-specific recommendations in PostgreSQL extension?
2. Should RFC 0053 (Stored Procedures) support type-specific variables?
3. How to test type-specific optimizations without running a real database?

**Out of Scope:**

- **User-defined types (UDTs)**: Too complex, future work
- **Type execution**: Ra doesn't execute queries, only optimizes
- **Type validation**: Assume schema is correct, focus on optimization
- **Nested object types**: Oracle object types with methods (complex, low priority)

## Future possibilities

### Natural Extensions

**1. Machine Learning Type Detection:**

- Automatically infer that a TEXT column contains JSON
- Suggest converting to JSONB for better performance

**2. Type-Specific Statistics Collection:**

- JSONB: Most common keys, average depth
- XML: Common XPath patterns
- Arrays: Length distribution, element type cardinality

**3. Type-Aware Join Optimization:**

- Optimize joins on JSONB fields (extract key, then join)
- Spatial joins with bounding box filters

**4. Cross-Database Type Migration:**

- Automatic schema conversion (Oracle → PostgreSQL)
- Data migration scripts with type mapping

**5. Type-Specific Compression:**

- Recommend dictionary compression for JSONB
- Suggest binary XML for Oracle XMLTYPE

### Long-term Vision

Ra becomes a **universal database type system** that:

- Understands all major database-specific types
- Provides optimal index recommendations for each type
- Enables seamless cross-database migration
- Offers type-aware cost modeling

Integration with other RFCs:

- **RFC 0053 (Stored Procedures)**: Type-specific variables and operators in procedures
- **RFC 0054 (Streaming Plans)**: Adjust plans when type-specific indexes are added
- **RFC 0056 (PostgreSQL Type Optimizations)**: Deep dive into PostgreSQL types
- **RFC 0057 (Cross-Database Type Adaptation)**: Advanced type mapping strategies

This RFC lays the groundwork for Ra to optimize the full spectrum of database types, not just traditional SQL types.
