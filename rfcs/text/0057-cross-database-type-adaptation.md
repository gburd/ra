# RFC 0057: Cross-Database Type Storage Adaptation

- Start Date: 2026-03-22
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: N/A

## Summary

Adapt Ra's optimizer behavior based on how different databases store the same logical type. This enables accurate optimization and cost modeling for types like JSON (stored differently in PostgreSQL, Oracle, MySQL) and spatial types (PostGIS vs Oracle Spatial), and provides cross-database migration assistance.

## Motivation

The same logical type (e.g., "JSON") is stored and indexed differently across databases:

**JSON Storage:**

- **PostgreSQL JSONB**: Binary format, parsed once, GIN indexes, O(1) key lookup
- **Oracle JSON**: Stored as CLOB (text), parsed on every access, function-based indexes required, no native GIN equivalent
- **MySQL JSON**: Binary format (similar to PostgreSQL), multi-valued indexes (MySQL 8.0+)
- **SQL Server**: No native JSON type (uses VARCHAR/NVARCHAR with JSON functions)

**Spatial Types:**

- **PostGIS (PostgreSQL)**: GEOMETRY/GEOGRAPHY types, GiST indexes, rich spatial functions
- **Oracle Spatial**: SDO_GEOMETRY type, R-tree indexes, different function names
- **MySQL**: GEOMETRY type, spatial indexes (MyISAM only until MySQL 8.0), limited functions
- **SQL Server**: GEOMETRY/GEOGRAPHY types, spatial indexes, different syntax

**Why this matters for optimization:**

1. **Cost model must reflect storage format**: Oracle JSON (CLOB) is 10x slower than PostgreSQL JSONB
2. **Index recommendations differ**: PostgreSQL uses GIN, Oracle uses function-based, MySQL uses multi-valued
3. **Query rewriting differs**: PostgreSQL `@>`, Oracle `JSON_EXISTS()`, MySQL `JSON_CONTAINS()`
4. **Migration planning**: Understanding storage differences helps estimate migration impact

This RFC provides database-specific cost models and optimization strategies for the same logical types.

## Guide-level explanation

### Example: JSON Query Across Databases

**Logical query** (database-agnostic):

```sql
SELECT id, data
FROM users
WHERE data contains {"status": "active"}
```

**PostgreSQL (JSONB):**

```sql
SELECT id, data
FROM users
WHERE data @> '{"status": "active"}';  -- Uses GIN index

-- Suggested index:
CREATE INDEX idx_users_data_gin ON users USING GIN (data);

-- Cost model: O(log N) with index, very fast
```

**Oracle (JSON as CLOB):**

```sql
SELECT id, data
FROM users
WHERE JSON_EXISTS(data, '$.status?(@ == "active")');

-- Suggested index:
CREATE INDEX idx_users_data_status ON users (JSON_VALUE(data, '$.status'));

-- Cost model: O(N) without function-based index, slow
-- JSON parsing on every row access
```

**MySQL (JSON):**

```sql
SELECT id, data
FROM users
WHERE JSON_CONTAINS(data, '"active"', '$.status');

-- Suggested index (MySQL 8.0+):
CREATE INDEX idx_users_data_status ON users ((CAST(data->>'$.status' AS CHAR(50))));

-- Cost model: O(log N) with index, but slower than PostgreSQL JSONB
```

**Ra's adaptation:**

```rust
match database {
    Database::PostgreSQL => {
        // Use GIN index, containment operator
        let cost = CostModel::JSONB_CONTAINMENT_WITH_INDEX;  // Very fast
        let sql = "data @> '{\"status\": \"active\"}'";
    }
    Database::Oracle => {
        // Use function-based index, JSON_EXISTS
        let cost = CostModel::JSON_CLOB_WITH_FUNCTION_INDEX;  // Slower (JSON parsing)
        let sql = "JSON_EXISTS(data, '$.status?(@ == \"active\")')";
        warn!("Oracle JSON is CLOB-based, consider migrating to PostgreSQL JSONB for better performance");
    }
    Database::MySQL => {
        // Use multi-valued index or expression index
        let cost = CostModel::MYSQL_JSON_WITH_INDEX;  // Medium performance
        let sql = "JSON_CONTAINS(data, '\"active\"', '$.status')";
    }
}
```

## Reference-level explanation

### Implementation Details

**Storage Adaptation Metadata:**

```rust
#[derive(Debug, Clone)]
pub struct TypeStorageInfo {
    pub logical_type: LogicalType,
    pub database: Database,
    pub physical_storage: PhysicalStorage,
    pub index_support: IndexSupport,
    pub cost_multiplier: f64,  // Relative to baseline (PostgreSQL)
}

#[derive(Debug, Clone)]
pub enum LogicalType {
    Json,
    Xml,
    Spatial,
    LargeText,
    LargeObject,
}

#[derive(Debug, Clone)]
pub enum PhysicalStorage {
    // JSON storage
    BinaryJson,              // PostgreSQL JSONB, MySQL JSON
    ClobWithConstraint,      // Oracle JSON (CLOB + IS JSON)
    PlainText,               // SQL Server (VARCHAR with JSON functions)

    // XML storage
    NativeXml,               // PostgreSQL XML, SQL Server XML
    BinaryXml,               // Oracle XMLTYPE (binary storage option)
    ClobXml,                 // Oracle XMLTYPE (CLOB storage)

    // Spatial storage
    PostGisGeometry,         // PostgreSQL + PostGIS extension
    OracleSpatialSdo,        // Oracle SDO_GEOMETRY
    SqlServerGeometry,       // SQL Server GEOMETRY/GEOGRAPHY
    MySqlGeometry,           // MySQL GEOMETRY

    // Large objects
    Toast,                   // PostgreSQL TOAST
    OracleLob,               // Oracle CLOB/BLOB
    SqlServerMax,            // SQL Server VARCHAR(MAX)/VARBINARY(MAX)
}

#[derive(Debug, Clone)]
pub struct IndexSupport {
    pub supported_indexes: Vec<IndexType>,
    pub default_recommendation: IndexType,
    pub requires_expression: bool,  // Function-based index needed?
}

#[derive(Debug, Clone)]
pub enum IndexType {
    // PostgreSQL
    Gin,
    Gist,
    BTree,
    Hash,

    // Oracle
    BTreeOracle,
    BitmapOracle,
    FunctionBased,
    XmlIndex,
    SpatialIndex,

    // MySQL
    BTreeMySQL,
    FullText,
    SpatialMySQL,
    MultiValued,  // MySQL 8.0+

    // SQL Server
    BTreeSqlServer,
    XmlPrimary,
    XmlSecondary,
    SpatialSqlServer,
}
```

**Storage Adaptation Rules:**

```rust
impl TypeStorageInfo {
    pub fn for_json(database: Database) -> Self {
        match database {
            Database::PostgreSQL => TypeStorageInfo {
                logical_type: LogicalType::Json,
                database: Database::PostgreSQL,
                physical_storage: PhysicalStorage::BinaryJson,
                index_support: IndexSupport {
                    supported_indexes: vec![IndexType::Gin, IndexType::BTree],
                    default_recommendation: IndexType::Gin,
                    requires_expression: false,  // GIN on column directly
                },
                cost_multiplier: 1.0,  // Baseline
            },

            Database::Oracle => TypeStorageInfo {
                logical_type: LogicalType::Json,
                database: Database::Oracle,
                physical_storage: PhysicalStorage::ClobWithConstraint,
                index_support: IndexSupport {
                    supported_indexes: vec![IndexType::FunctionBased],
                    default_recommendation: IndexType::FunctionBased,
                    requires_expression: true,  // Index on JSON_VALUE(col, '$.path')
                },
                cost_multiplier: 10.0,  // 10x slower due to CLOB storage + parsing
            },

            Database::MySQL => TypeStorageInfo {
                logical_type: LogicalType::Json,
                database: Database::MySQL,
                physical_storage: PhysicalStorage::BinaryJson,
                index_support: IndexSupport {
                    supported_indexes: vec![IndexType::MultiValued, IndexType::BTreeMySQL],
                    default_recommendation: IndexType::MultiValued,
                    requires_expression: true,  // Expression index on JSON path
                },
                cost_multiplier: 2.0,  // 2x slower than PostgreSQL (less mature)
            },

            Database::SQLServer => TypeStorageInfo {
                logical_type: LogicalType::Json,
                database: Database::SQLServer,
                physical_storage: PhysicalStorage::PlainText,
                index_support: IndexSupport {
                    supported_indexes: vec![IndexType::BTreeSqlServer],
                    default_recommendation: IndexType::BTreeSqlServer,
                    requires_expression: true,  // Computed column with index
                },
                cost_multiplier: 15.0,  // 15x slower (no native JSON type)
            },
        }
    }
}
```

**Cost Model Adaptation:**

```rust
impl CostModel {
    pub fn estimate_json_access_cost(
        &self,
        storage_info: &TypeStorageInfo,
        operation: JsonOperation,
        has_index: bool,
    ) -> Cost {
        let base_cost = match operation {
            JsonOperation::Containment => self.json_containment_cost,
            JsonOperation::PathExtraction => self.json_extraction_cost,
            JsonOperation::Full Parse => self.json_parse_cost,
        };

        // Apply storage multiplier
        let storage_adjusted = base_cost * storage_info.cost_multiplier;

        // Index adjustment
        if has_index {
            match storage_info.physical_storage {
                PhysicalStorage::BinaryJson => storage_adjusted * 0.01,  // 100x speedup with GIN
                PhysicalStorage::ClobWithConstraint => storage_adjusted * 0.2,  // 5x speedup with function-based index
                PhysicalStorage::PlainText => storage_adjusted * 0.5,  // 2x speedup with computed column index
            }
        } else {
            storage_adjusted
        }
    }
}
```

**Cross-Database Query Translation:**

```rust
pub struct QueryTranslator {
    source_db: Database,
    target_db: Database,
}

impl QueryTranslator {
    pub fn translate_json_predicate(&self, expr: &Expr) -> Result<Expr, TranslationError> {
        match (self.source_db, self.target_db, expr) {
            // PostgreSQL -> Oracle
            (Database::PostgreSQL, Database::Oracle, Expr::BinOp {
                op: Op::JsonContains,
                left: col,
                right: json_value,
            }) => {
                // Translate: data @> '{"key": "value"}'
                // To: JSON_EXISTS(data, '$.key?(@ == "value")')
                Ok(Expr::Function {
                    name: "JSON_EXISTS".to_string(),
                    args: vec![
                        col.clone(),
                        self.jsonb_to_jsonpath(json_value)?,
                    ],
                })
            }

            // Oracle -> PostgreSQL
            (Database::Oracle, Database::PostgreSQL, Expr::Function {
                name: "JSON_VALUE",
                args: vec![col, path],
            }) => {
                // Translate: JSON_VALUE(data, '$.key')
                // To: data->>'key'
                Ok(Expr::JsonExtractText {
                    object: Box::new(col.clone()),
                    path: self.extract_json_path(path)?,
                })
            }

            // MySQL -> PostgreSQL
            (Database::MySQL, Database::PostgreSQL, Expr::Function {
                name: "JSON_CONTAINS",
                ..
            }) => {
                // Translate MySQL JSON_CONTAINS to PostgreSQL @>
                todo!()
            }

            _ => Err(TranslationError::UnsupportedTranslation {
                source_db: self.source_db,
                target_db: self.target_db,
                expr: expr.clone(),
            }),
        }
    }
}
```

**Migration Impact Analysis:**

```rust
pub struct MigrationImpact {
    pub source_db: Database,
    pub target_db: Database,
    pub type_conversions: Vec<TypeConversion>,
    pub index_changes: Vec<IndexChange>,
    pub estimated_performance_change: f64,  // Multiplier (0.5 = 2x slower, 2.0 = 2x faster)
    pub warnings: Vec<String>,
}

pub struct TypeConversion {
    pub table: String,
    pub column: String,
    pub source_type: Type,
    pub target_type: Type,
    pub storage_change: StorageChange,
}

#[derive(Debug, Clone)]
pub enum StorageChange {
    Improved { reason: String, speedup: f64 },
    Degraded { reason: String, slowdown: f64 },
    Similar { note: String },
    Manual { action_required: String },
}

impl MigrationAnalyzer {
    pub fn analyze_json_migration(&self, source_db: Database, target_db: Database) -> MigrationImpact {
        let source_info = TypeStorageInfo::for_json(source_db);
        let target_info = TypeStorageInfo::for_json(target_db);

        let performance_change = target_info.cost_multiplier / source_info.cost_multiplier;

        let warnings = match (source_db, target_db) {
            (Database::PostgreSQL, Database::Oracle) => vec![
                "PostgreSQL JSONB -> Oracle JSON (CLOB): 10x performance degradation expected".to_string(),
                "GIN indexes -> function-based indexes: manual conversion required".to_string(),
                "Containment operators (@>) -> JSON_EXISTS(): syntax rewrite needed".to_string(),
            ],
            (Database::Oracle, Database::PostgreSQL) => vec![
                "Oracle JSON (CLOB) -> PostgreSQL JSONB: 10x performance improvement expected".to_string(),
                "Function-based indexes -> GIN indexes: recommend creating GIN indexes".to_string(),
                "JSON_VALUE() -> ->> operator: automatic conversion possible".to_string(),
            ],
            (Database::MySQL, Database::PostgreSQL) => vec![
                "MySQL JSON -> PostgreSQL JSONB: 2x performance improvement expected".to_string(),
                "Multi-valued indexes -> GIN indexes: recommend GIN indexes".to_string(),
                "JSON_CONTAINS() -> @> operator: automatic conversion".to_string(),
            ],
            _ => vec![],
        };

        MigrationImpact {
            source_db,
            target_db,
            type_conversions: vec![/* ... */],
            index_changes: vec![/* ... */],
            estimated_performance_change: performance_change,
            warnings,
        }
    }
}
```

### Integration Points

**1. Cost Model (ra-core):**

Add storage-aware cost estimates for all operations on typed columns.

**2. Index Advisor (RFC 0021):**

Provide database-specific index recommendations based on storage format.

**3. Query Parser:**

Parse database-specific syntax for the same logical operation.

**4. Migration Tools:**

Generate migration scripts with type conversions and index changes.

**5. Stored Procedures (RFC 0053):**

Handle type-specific operators in stored procedures during cross-database analysis.

### Error Handling

```rust
#[derive(Debug, Error)]
pub enum StorageAdaptationError {
    #[error("Unsupported type conversion: {source_type} ({source_db}) -> {target_type} ({target_db})")]
    UnsupportedConversion {
        source_type: Type,
        source_db: Database,
        target_type: Type,
        target_db: Database,
    },

    #[error("No equivalent storage format in {target_db} for {logical_type}")]
    NoEquivalent {
        logical_type: LogicalType,
        target_db: Database,
    },

    #[error("Manual intervention required for {reason}")]
    ManualInterventionRequired { reason: String },
}
```

### Performance Considerations

**Lookup Overhead:**

- TypeStorageInfo lookup per column: O(1) with HashMap
- Cache results per (database, type) tuple

**Cost Model Accuracy:**

- Storage multipliers are estimates (may vary by workload)
- Requires benchmarking on target database for accuracy

**Translation Overhead:**

- Query translation is O(N) in query size
- Only performed during migration analysis (not runtime)

## Drawbacks

**Maintenance Burden:**

- Must track storage differences across databases
- Database updates may change storage formats
- Cost multipliers require periodic re-calibration

**Incomplete Coverage:**

- Cannot handle all type differences (some are too database-specific)
- Edge cases require manual intervention

**Estimation Uncertainty:**

- Cost multipliers are rough estimates
- Actual performance varies by workload, data distribution, hardware

**False Precision:**

- "10x slower" is approximate, may mislead users
- Should present as ranges (5-15x) not exact numbers

## Rationale and alternatives

### Why This Design?

**Storage-Aware Cost Model:**

- Reflects reality (JSON in Oracle is slow)
- Helps users make informed decisions

**Migration Impact Analysis:**

- Users need to understand performance implications of migrations
- Proactive warnings prevent post-migration surprises

**Database-Specific Optimization:**

- Each database has unique strengths (GIN indexes in PostgreSQL)
- Optimizer should leverage them

### Alternative Approaches

**1. Ignore Storage Differences:**

- Treat all JSON types as equivalent
- **Rejected**: Cost model would be inaccurate

**2. Single Optimal Database:**

- Only optimize for PostgreSQL (best type support)
- **Rejected**: Users may be locked into Oracle/MySQL

**3. Runtime Benchmarking:**

- Measure actual performance, don't estimate
- **Rejected**: Too slow, requires live database

**4. User-Provided Multipliers:**

- Users specify cost multipliers
- **Rejected**: Too complex, error-prone

### Impact of Not Doing This

**Without storage adaptation:**

- Cost model inaccurate for cross-database queries
- Index recommendations may not work (suggest GIN for Oracle)
- Migration planning is blind (no performance estimates)
- Optimizations may degrade performance on some databases

**Workaround:**

- Database-specific query hints
- Manual performance testing before migration
- Conservative optimization (assume worst-case)

## Prior art

### Academic Research

**Cross-Database Query Optimization:**

- [Schema-Agnostic Indexing for Faster Query Processing](https://dl.acm.org/doi/10.1145/3318464.3389754) - Indexing across storage formats
- [Automatic Database Management System Tuning Through Large-scale Machine Learning](https://dl.acm.org/doi/10.1145/3035918.3064029) - Learning cost models from workloads

### Industry Solutions

**AWS Database Migration Service (DMS):**

- Handles type conversions (Oracle -> PostgreSQL)
- Schema Conversion Tool provides warnings
- Does not optimize queries, only converts schema

**Azure Data Migration Assistant:**

- Analyzes SQL Server -> Azure SQL migrations
- Identifies incompatibilities
- Does not provide performance estimates

**Ispirer SQLWays:**

- Converts queries between databases
- Handles type mapping (CLOB -> TEXT)
- Limited optimization, mostly syntax translation

**Oracle GoldenGate:**

- Real-time replication across databases
- Handles type conversions at data level
- No query optimization

**What We Can Learn:**

- Type mapping is well-understood (many tools do it)
- Performance estimation is rare (Ra provides unique value)
- Cross-database optimization is largely unexplored
- Users want proactive warnings about performance implications

## Unresolved questions

**Design Questions:**

1. Should cost multipliers be user-configurable?
2. How to present uncertainty in cost estimates? (Ranges? Confidence intervals?)
3. Should Ra support automatic query translation for common patterns?

**Implementation Questions:**

1. How to calibrate cost multipliers? (Benchmarks on each database)
2. How to handle database version differences? (PostgreSQL 12 vs 16 JSONB improvements)
3. Should storage info be cached? For how long?

**Integration Questions:**

1. How to integrate with migration tools? (Export migration scripts?)
2. Should Ra provide a "migration readiness score"?
3. How to visualize storage differences for users?

**Out of Scope:**

- **Data migration**: Ra optimizes queries, doesn't migrate data
- **Schema design**: Suggesting schema changes (future work)
- **Automatic code rewriting**: Only analysis, not execution

## Future possibilities

### Natural Extensions

**1. Learned Cost Models:**

- Train on workload data from each database
- Replace hard-coded multipliers with ML models
- Adapt to hardware differences (SSD vs HDD)

**2. Automatic Query Rewriting for Migration:**

- Given PostgreSQL query, generate equivalent Oracle query
- Handle syntax differences automatically
- Provide confidence scores

**3. Storage Format Recommendations:**

- "Your Oracle JSON columns would be 10x faster in PostgreSQL JSONB"
- Suggest database switches for performance-critical workloads

**4. Hybrid Database Optimization:**

- Query spans multiple databases (Oracle + PostgreSQL)
- Optimize where to execute each part
- Federated query optimization

**5. Continuous Calibration:**

- Monitor actual query performance
- Update cost multipliers based on observations
- Adapt to changing workloads

### Long-term Vision

Ra becomes a **universal cross-database optimizer** that:

- Understands storage formats for all major databases
- Provides accurate performance estimates for migrations
- Automatically rewrites queries for target database
- Recommends optimal database for each workload

Integration with other RFCs:

- **RFC 0053 (Stored Procedures)**: Cross-database procedure analysis
- **RFC 0054 (Streaming Plans)**: Adapt to storage changes
- **RFC 0055 (Type Support)**: Foundation for storage-aware optimization
- **RFC 0056 (PostgreSQL Optimizations)**: Deep PostgreSQL storage knowledge

This RFC enables Ra to optimize queries correctly regardless of underlying storage format, and helps users navigate the complex landscape of database-specific type implementations.
