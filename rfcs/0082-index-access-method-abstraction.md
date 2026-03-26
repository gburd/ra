# RFC 0082: Index Access Method Abstraction

- Start Date: 2026-03-26
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should use a database-agnostic index abstraction layer that discovers
index capabilities at runtime instead of hardcoding index types (GIN, RUM,
B-tree, etc.) in optimization rules. This allows rules to work across
different databases (PostgreSQL, DocumentDB) and automatically adapt when
new index types become available.

## Motivation

Ra's current optimization rules hardcode specific index types, making them
brittle and database-specific. For example, `gin-index-for-arrays.rra`
checks `has_gin_index_on(table, col)`, which only works for PostgreSQL GIN
indexes and breaks when:

1. **DocumentDB uses RUM fork** instead of GIN for BSON indexing
2. **New index types** are added (e.g., PostgreSQL 16+ improvements)
3. **Cross-database optimization** is needed (same rule for PG and DocumentDB)
4. **Index type is irrelevant** to the optimization (only capability matters)

### Current Problem

**Hardcoded index type checks:**

```rust
// rules/physical/index-selection/gin-index-for-arrays.rra
rw!("gin-index-for-array-contains";
    "(filter (array-contains ?col ?elements) (scan ?table))" =>
    "(gin-index-scan ?index ?col array-contains ?elements)"
    if has_gin_index_on("?table", "?col")  // ← Hardcoded GIN
),
```

This rule fails when:
- DocumentDB uses RUM fork (not GIN)
- PostgreSQL installs RUM extension (better than GIN for some queries)
- Future PostgreSQL adds better inverted index

**Database-specific cost models embedded in rules:**

```rust
// Cost model hardcoded for GIN
let posting_scan = selectivity * table_rows * 0.5;  // GIN constant
```

DocumentDB RUM has different cost characteristics (wider postings, distance
ordering), but rules don't adapt.

### Desired Behavior

**Generic capability-based checks:**

```rust
// rules/physical/index-selection/inverted-index-for-arrays.rra
rw!("inverted-index-for-array-contains";
    "(filter (array-contains ?col ?elements) (scan ?table))" =>
    "(index-scan ?index ?col array-contains ?elements)"
    if has_index_supporting("?table", "?col", IndexOperation::ArrayContainment)
),
```

The optimizer discovers at runtime:
- PostgreSQL GIN → use GIN
- PostgreSQL RUM → use RUM (potentially better for ranked queries)
- DocumentDB RUM → use DocumentDB RUM (BSON-specific)

**Database-specific cost models discovered at runtime:**

```rust
let indexes = find_indexes_supporting(table, col, IndexOperation::ArrayContainment);
let best_index = indexes.iter()
    .min_by_key(|idx| idx.estimate_scan_cost(selectivity, table_rows, limit));

// Cost model is index-type-specific, discovered automatically
match best_index.access_method {
    IndexAccessMethod::GIN => /* GIN cost model */,
    IndexAccessMethod::RUM => /* RUM cost model */,
    IndexAccessMethod::DocumentDBRUM => /* DocumentDB RUM cost model */,
}
```

### Expected Impact

| Scenario | Current Behavior | With Abstraction | Gain |
|----------|-----------------|------------------|------|
| DocumentDB array query | Sequential scan (no GIN) | RUM scan detected | 10-100x |
| PostgreSQL with RUM | Uses GIN (suboptimal) | Uses RUM when beneficial | 2-50x for ranked queries |
| New index type added | Rules must be rewritten | Automatic support | Zero refactoring |
| Cross-database rules | Separate rule files per DB | Single generic rule | Reduced code duplication |

## Guide-level explanation

### Index Capability Discovery

Instead of asking "Does this table have a GIN index?", rules ask "Does this
table have any index supporting array containment?".

**Old (hardcoded)**:
```rust
if has_gin_index_on("articles", "tags") {
    // Use GIN scan
}
```

**New (capability-based)**:
```rust
let indexes = find_indexes_supporting(
    "articles",
    "tags",
    IndexOperation::ArrayContainment,
);
if let Some(best_index) = indexes.first() {
    // Use whatever index type is installed
    // Optimizer chooses based on capabilities and cost
}
```

### Index Operations Taxonomy

The abstraction defines database-agnostic operations that rules can query:

```rust
pub enum IndexOperation {
    /// Array containment operators (@>, &&, <@).
    ArrayContainment,
    /// JSONB/JSON containment operators (@>, ?).
    JsonContainment,
    /// Full-text search (@@, to_tsquery).
    FullTextSearch,
    /// Phrase search with positions (<->).
    PhraseSearch,
    /// Spatial containment (ST_Contains, ST_Within).
    SpatialContainment,
    /// Geospatial distance ordering (<->).
    GeospatialDistance,
    /// K-nearest-neighbor search.
    KNNSearch,
    /// JSON path extraction.
    JsonPath,
    /// Equality on scalar values.
    ScalarEquality,
    /// Range scan on scalar values.
    ScalarRange,
}
```

### Automatic Index Type Detection

The optimizer queries the database catalog to discover indexes:

```sql
-- PostgreSQL catalog query
SELECT
    i.relname AS index_name,
    t.relname AS table_name,
    a.amname AS access_method,        -- 'gin', 'rum', 'btree', etc.
    opf.opfname AS operator_family,   -- 'array_ops', 'rum_tsvector_ops', etc.
    array_agg(att.attname) AS columns
FROM pg_index idx
JOIN pg_class i ON i.oid = idx.indexrelid
JOIN pg_class t ON t.oid = idx.indrelid
JOIN pg_am a ON a.oid = i.relam
JOIN pg_opclass opc ON opc.oid = ANY(idx.indclass)
JOIN pg_opfamily opf ON opf.oid = opc.opcfamily
...
```

From this metadata, Ra constructs:

```rust
IndexMetadata {
    name: "idx_articles_tags",
    table: "articles",
    columns: ["tags"],
    access_method: IndexAccessMethod::GIN,      // Discovered from pg_am.amname
    operator_family: "array_ops",               // From pg_opfamily
    capabilities: IndexCapabilities {
        supports_containment: true,             // Derived from opfamily
        supports_distance_ordering: false,      // GIN doesn't support ordering
        supports_phrase_search: false,          // GIN requires heap recheck
        cost_factors: IndexCostFactors::gin_default(),
    },
    statistics: IndexStats { ... },
}
```

### Rule Composition

Rules automatically compose when capabilities are discovered:

**Query:**
```sql
SELECT * FROM articles
WHERE tags @> ARRAY['optimization']
ORDER BY created_at DESC;
```

**With GIN index:**
1. `inverted-index-for-containment` → use GIN for @>
2. `sort` → external sort needed (GIN doesn't provide ordering)

**With RUM index:**
1. `inverted-index-for-containment` → use RUM for @>
2. `eliminate-sort-for-ordered-index` → RUM provides ordering, no sort needed

**With DocumentDB RUM:**
1. `inverted-index-for-containment` → use DocumentDB RUM for BSON containment
2. `eliminate-sort-for-ordered-index` → RUM provides ordering
3. BSON-specific cost model applied automatically

### Cost Model Selection

Each index type has its own cost model, discovered at runtime:

```rust
impl IndexMetadata {
    pub fn estimate_scan_cost(
        &self,
        selectivity: f64,
        table_rows: u64,
        limit: Option<u64>,
    ) -> f64 {
        match self.access_method {
            IndexAccessMethod::GIN => {
                // Standard GIN: narrower postings, no ordering
                let posting_scan = effective_rows * 0.5;
                lookup_cost + posting_scan + tuple_fetch
            }
            IndexAccessMethod::RUM => {
                // PostgreSQL RUM: wider postings, supports ordering
                let posting_scan = effective_rows * 0.7;
                let ordering_benefit = if query.has_order_by() { 0.3 } else { 1.0 };
                (lookup_cost + posting_scan + tuple_fetch) * ordering_benefit
            }
            IndexAccessMethod::DocumentDBRUM => {
                // DocumentDB RUM: BSON-specific, wider postings
                let posting_scan = effective_rows * 0.7;
                let bson_overhead = 1.1; // BSON deserialization
                (lookup_cost + posting_scan + tuple_fetch) * bson_overhead
            }
        }
    }
}
```

The optimizer automatically picks the cheapest index for the query:

```rust
let indexes = find_indexes_supporting(table, col, IndexOperation::ArrayContainment);
let best_index = indexes.iter()
    .min_by_key(|idx| {
        idx.estimate_scan_cost(selectivity, table_rows, query.limit)
    })
    .expect("at least one index supports operation");
```

## Reference-level explanation

### Index Access Method Taxonomy

The abstraction defines a database-agnostic taxonomy of index access methods:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexAccessMethod {
    /// B-tree or B+ tree (ordered, range scans).
    BTree,
    /// Hash index (equality only).
    Hash,
    /// Generalized Inverted Index (PostgreSQL GIN).
    GIN,
    /// GIN extension with distance ordering (PostgreSQL RUM).
    RUM,
    /// DocumentDB's RUM fork (BSON-specific).
    DocumentDBRUM,
    /// Generalized Search Tree (PostgreSQL GiST).
    GiST,
    /// Block Range Index (PostgreSQL BRIN).
    BRIN,
    /// Bloom filter index.
    Bloom,
    /// R-tree for spatial data.
    RTree,
    /// Column-oriented storage index.
    Columnstore,
    /// Bitmap index.
    Bitmap,
    /// Full-text search index.
    FullText,
}
```

Each access method is mapped from database-specific names:

```rust
impl IndexAccessMethod {
    pub fn from_pg_amname(amname: &str) -> Option<Self> {
        match amname {
            "btree" => Some(Self::BTree),
            "hash" => Some(Self::Hash),
            "gin" => Some(Self::GIN),
            "rum" => Some(Self::RUM),
            "gist" => Some(Self::GiST),
            "brin" => Some(Self::BRIN),
            _ => None,
        }
    }
}
```

### Index Capabilities

Capabilities are derived from the combination of access method and operator
family:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct IndexCapabilities {
    pub supports_ordered_scan: bool,
    pub supports_bitmap_scan: bool,
    pub supports_point_lookup: bool,
    pub supports_range_scan: bool,
    pub supports_containment: bool,
    pub supports_distance_ordering: bool,
    pub supports_full_text: bool,
    pub supports_phrase_search: bool,
    pub supports_spatial: bool,
    pub supports_json_path: bool,
    pub cost_factors: IndexCostFactors,
}
```

**Example mappings:**

| Access Method | Operator Family | Capabilities |
|--------------|----------------|--------------|
| GIN | array_ops | containment=yes, ordering=no, phrase=no |
| RUM | rum_tsvector_ops | containment=no, ordering=yes, phrase=yes, fulltext=yes |
| RUM | rum_anyarray_ops | containment=yes, ordering=no, phrase=no |
| DocumentDB RUM | bson_extended_rum_single_path_ops | containment=yes, ordering=yes, json_path=yes, fulltext=yes |
| GiST | gist_geometry_ops_2d | spatial=yes, distance_ordering=yes |
| BTree | btree_ops | ordered_scan=yes, range_scan=yes, point_lookup=yes |
| Hash | hash_ops | point_lookup=yes, everything_else=no |

### Capability Derivation Logic

```rust
impl IndexCapabilities {
    pub fn from_access_method_and_opfamily(
        access_method: IndexAccessMethod,
        opfamily: &str,
    ) -> Self {
        match (access_method, opfamily) {
            // PostgreSQL GIN for arrays
            (IndexAccessMethod::GIN, "array_ops") => Self {
                supports_containment: true,
                supports_distance_ordering: false,
                supports_phrase_search: false,
                cost_factors: IndexCostFactors::gin_default(),
                ...
            },

            // PostgreSQL RUM for full-text
            (IndexAccessMethod::RUM, "rum_tsvector_ops") => Self {
                supports_full_text: true,
                supports_phrase_search: true,      // RUM: in-index verification
                supports_distance_ordering: true,  // RUM: distance operator
                supports_ordered_scan: true,
                cost_factors: IndexCostFactors::rum_default(),
                ...
            },

            // DocumentDB RUM for BSON
            (IndexAccessMethod::DocumentDBRUM, "bson_extended_rum_single_path_ops") => Self {
                supports_containment: true,
                supports_distance_ordering: true,
                supports_json_path: true,
                supports_full_text: true,          // $text search
                cost_factors: IndexCostFactors::rum_default(),
                ...
            },

            // Fallback: use access method defaults
            _ => Self::default_for(access_method),
        }
    }
}
```

### Index Discovery

The optimizer queries the database catalog to discover indexes:

```rust
pub fn discover_indexes_for_table(
    connection: &Connection,
    table: &str,
) -> Vec<IndexMetadata> {
    // Query pg_index, pg_class, pg_am, pg_opfamily
    let query = r#"
        SELECT
            i.relname AS index_name,
            t.relname AS table_name,
            a.amname AS access_method,
            opf.opfname AS operator_family,
            array_agg(att.attname ORDER BY k.i) AS columns,
            pg_relation_size(i.oid) AS index_size,
            idx.indnatts AS num_columns,
            ...
        FROM pg_index idx
        JOIN pg_class i ON i.oid = idx.indexrelid
        JOIN pg_class t ON t.oid = idx.indrelid
        JOIN pg_am a ON a.oid = i.relam
        JOIN pg_opclass opc ON opc.oid = ANY(idx.indclass)
        JOIN pg_opfamily opf ON opf.oid = opc.opcfamily
        CROSS JOIN LATERAL unnest(idx.indkey::int[]) WITH ORDINALITY AS k(attnum, i)
        JOIN pg_attribute att ON att.attrelid = t.oid AND att.attnum = k.attnum
        WHERE t.relname = $1
        GROUP BY i.relname, t.relname, a.amname, opf.opfname, ...
    "#;

    let rows = connection.query(query, &[&table])?;
    rows.iter().map(|row| {
        let access_method = IndexAccessMethod::from_pg_amname(
            row.get("access_method")
        ).unwrap_or(IndexAccessMethod::BTree);

        let operator_family: String = row.get("operator_family");
        let capabilities = IndexCapabilities::from_access_method_and_opfamily(
            access_method,
            &operator_family,
        );

        IndexMetadata {
            name: row.get("index_name"),
            table: row.get("table_name"),
            columns: row.get("columns"),
            access_method,
            operator_family,
            capabilities,
            statistics: IndexStats::from_catalog(connection, &row.get("index_name"))?,
        }
    }).collect()
}
```

### Operation Support Checking

```rust
pub fn find_indexes_supporting(
    connection: &Connection,
    table: &str,
    column: &str,
    operation: &IndexOperation,
) -> Vec<IndexMetadata> {
    discover_indexes_for_table(connection, table)
        .into_iter()
        .filter(|idx| {
            idx.columns.contains(&column.to_string())
                && idx.capabilities.supports_operation(operation)
        })
        .collect()
}

impl IndexCapabilities {
    pub fn supports_operation(&self, op: &IndexOperation) -> bool {
        match op {
            IndexOperation::ArrayContainment => self.supports_containment,
            IndexOperation::FullTextSearch => self.supports_full_text,
            IndexOperation::PhraseSearch => self.supports_phrase_search,
            IndexOperation::KNNSearch => self.supports_distance_ordering,
            IndexOperation::SpatialContainment => self.supports_spatial,
            IndexOperation::ScalarRange => self.supports_range_scan,
            IndexOperation::ScalarEquality => self.supports_point_lookup,
            ...
        }
    }
}
```

### Cost Estimation

Each index metadata includes its own cost model:

```rust
impl IndexMetadata {
    pub fn estimate_scan_cost(
        &self,
        selectivity: f64,
        table_rows: u64,
        limit: Option<u64>,
    ) -> f64 {
        let matching_rows = (table_rows as f64 * selectivity).max(1.0);
        let effective_rows = match limit {
            Some(k) => (k as f64 * 1.2).min(matching_rows),  // RUM benefit
            None => matching_rows,
        };

        match self.access_method {
            IndexAccessMethod::BTree => {
                let tree_traversal = self.statistics.levels as f64 * 4.0;
                let scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = if self.capabilities.cost_factors.covering {
                    0.0
                } else {
                    effective_rows * self.capabilities.cost_factors.tuple_fetch_cost
                };
                tree_traversal + scan + fetch
            }

            IndexAccessMethod::GIN => {
                let posting_scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                self.capabilities.cost_factors.lookup_cost + posting_scan + fetch
            }

            IndexAccessMethod::RUM | IndexAccessMethod::DocumentDBRUM => {
                // RUM benefits from limit due to distance ordering
                let posting_scan = effective_rows * self.capabilities.cost_factors.range_scan_cost;
                let fetch = effective_rows * self.capabilities.cost_factors.tuple_fetch_cost;
                self.capabilities.cost_factors.lookup_cost + posting_scan + fetch
            }

            IndexAccessMethod::GiST => {
                let tree_traversal = self.statistics.levels as f64 * 2.0;
                let node_checks = effective_rows * 0.5;  // Bounding box checks
                let recheck = effective_rows * 5.0;      // Exact geometry tests
                tree_traversal + node_checks + recheck
            }

            // ... other index types
        }
    }
}
```

### Rule Migration

**Old rule (hardcoded GIN)**:

```rust
// rules/physical/index-selection/gin-index-for-arrays.rra
rw!("gin-index-for-array-contains";
    "(filter (array-contains ?col ?elements) (scan ?table))" =>
    "(gin-index-scan ?index ?col array-contains ?elements)"
    if has_gin_index_on("?table", "?col")
),
```

**New rule (generic inverted index)**:

```rust
// rules/physical/index-selection/inverted-index-for-arrays.rra
rw!("inverted-index-for-array-contains";
    "(filter (array-contains ?col ?elements) (scan ?table))" =>
    "(index-scan ?index ?col array-contains ?elements)"
    if has_index_supporting("?table", "?col", IndexOperation::ArrayContainment)
),
```

**Implementation of `has_index_supporting`:**

```rust
fn has_index_supporting(
    table_var: Var,
    column_var: Var,
    operation: IndexOperation,
) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
    move |egraph, _id, subst| {
        let table = extract_string(egraph, subst[table_var]);
        let column = extract_string(egraph, subst[column_var]);

        let indexes = find_indexes_supporting(
            egraph.analysis.connection(),
            &table,
            &column,
            &operation,
        );

        !indexes.is_empty()
    }
}
```

### Extension Points

The abstraction allows new index types to be added without changing rules:

**Add support for PostgreSQL 17's new hypothetical "SuperGIN":**

```rust
// In index_metadata.rs
impl IndexAccessMethod {
    pub fn from_pg_amname(amname: &str) -> Option<Self> {
        match amname {
            ...
            "supergin" => Some(Self::SuperGIN),  // New index type
            ...
        }
    }
}

impl IndexCapabilities {
    pub fn from_access_method_and_opfamily(
        access_method: IndexAccessMethod,
        opfamily: &str,
    ) -> Self {
        match (access_method, opfamily) {
            ...
            (IndexAccessMethod::SuperGIN, _) => Self {
                supports_containment: true,
                supports_distance_ordering: true,  // New capability!
                cost_factors: IndexCostFactors {
                    lookup_cost: 2.5,    // Faster than GIN
                    range_scan_cost: 0.3,
                    ...
                },
                ...
            },
            ...
        }
    }
}
```

**Existing rules automatically start using SuperGIN**:
- No rule changes needed
- Optimizer discovers SuperGIN from catalog
- Cost model compares SuperGIN vs GIN vs RUM
- Best index is selected automatically

## Drawbacks

1. **Catalog query overhead**: Discovering indexes at planning time adds a
   catalog query. Mitigation: cache index metadata per connection.

2. **Loss of explicitness**: Rules no longer explicitly mention "GIN" or "RUM",
   which may make them harder to understand. Mitigation: comprehensive
   documentation and capability taxonomy.

3. **Capability mapping maintenance**: New operator families require updating
   capability derivation logic. Mitigation: default to conservative capabilities
   for unknown operator families.

4. **Testing complexity**: Must test with multiple index types to ensure rules
   work correctly. Mitigation: parameterized tests with different index configs.

## Rationale and alternatives

### Why not keep database-specific rules?

**Option rejected**: Maintain separate rule files per database:
- `gin-index-for-arrays.rra` (PostgreSQL only)
- `rum-index-for-arrays.rra` (PostgreSQL with RUM only)
- `documentdb-rum-index-for-arrays.rra` (DocumentDB only)

**Rejected because**:
- Massive code duplication (same rule logic, different index names)
- Maintenance burden (fix bug in 3 places)
- Doesn't compose (can't have GIN and RUM both installed)
- Breaks when new index types are added

### Why not runtime plugin system?

**Option rejected**: Allow databases to register custom index types as plugins:

```rust
index_registry.register("rum", RumIndexPlugin {
    supports_operations: [ArrayContainment, FullTextSearch],
    cost_model: |...| { ... },
});
```

**Rejected because**:
- Adds significant complexity (plugin API, dynamic loading)
- Still requires explicit registration for every new index type
- Doesn't leverage existing database catalogs
- Harder to reason about which plugins are active

### Why catalog-based discovery?

**Chosen approach**: Query database catalogs to discover indexes at runtime.

**Benefits**:
- Zero configuration: indexes are discovered automatically
- Always accurate: reflects actual database state
- Composable: multiple index types can coexist
- Extensible: new index types work immediately
- Database-agnostic: same approach works for PG, DocumentDB, etc.

## Prior art

### PostgreSQL planner's index selection

PostgreSQL's planner uses a similar abstraction internally:

```c
// src/backend/optimizer/path/indxpath.c
typedef struct {
    IndexOptInfo *index;
    List *indexclauses;
    bool indexonly;
    double selectivity;
} IndexPath;

// Checks index access method capabilities
if (index->relam == GIN_AM_OID) {
    // GIN-specific logic
} else if (index->relam == RUM_AM_OID) {
    // RUM-specific logic
}
```

Ra's abstraction improves on this by:
- Making capabilities explicit (not AMHandler function pointers)
- Exposing capabilities to rules (not buried in C code)
- Supporting cross-database compatibility

### Apache Calcite's index metadata

Calcite uses a similar concept with `RelOptTable.getStatistic()` and
`RelOptTable.getIndexes()`:

```java
interface IndexMetadata {
    String getName();
    List<String> getColumns();
    IndexType getType();  // BTREE, HASH, BITMAP, etc.
    boolean supportsOrdering();
    boolean supportsLookup();
}
```

Ra's design differs by:
- More granular capabilities (containment, distance ordering, phrase search)
- Database-specific cost models per index type
- Automatic derivation from operator families

### Microsoft SQL Server indexed views

SQL Server's indexed views are another form of index abstraction:

```sql
CREATE INDEX idx_view ON my_view(col1, col2) WITH (...)
```

The optimizer treats indexed views as just another index type. Ra's design
extends this to all index types, not just views.

## Unresolved questions

1. **Catalog caching strategy**: How long should discovered index metadata be
   cached? Per query? Per connection? Global cache with invalidation?

2. **Operator family standardization**: Should Ra define a canonical set of
   operation classes and map database-specific operator families to them?

3. **Cross-database compatibility**: How should Ra handle index types that exist
   in one database but not another (e.g., DocumentDB RUM on standard PostgreSQL)?

4. **Custom index types**: How should users register custom index types that Ra
   doesn't know about? Should there be a configuration file?

5. **Performance impact**: What is the overhead of catalog queries for index
   discovery? Does caching eliminate this overhead?

## Future possibilities

1. **Index recommendation engine**: Use capability taxonomy to recommend index
   types based on query workload. "You're using GIN for ranked text search,
   consider RUM for 10x improvement."

2. **Cross-database index migration**: Automatically suggest equivalent indexes
   when migrating from PostgreSQL to DocumentDB (GIN → DocumentDB RUM).

3. **Index type experimentation**: Add hypothetical index types for "what-if"
   analysis. "What if PostgreSQL added a GIN variant with ordering?"

4. **Capability learning**: Learn index capabilities from query execution
   feedback rather than hardcoding them.

5. **Unified index cost model**: Use machine learning to learn cost models
   for new index types automatically.

## Implementation plan

### Phase 1: Core abstraction (2-3 weeks)

1. Implement `IndexAccessMethod`, `IndexOperation`, `IndexCapabilities`
2. Implement catalog discovery for PostgreSQL
3. Add `discover_indexes_for_table()`, `find_indexes_supporting()`
4. Write unit tests for capability derivation

### Phase 2: Rule migration (2-3 weeks)

1. Refactor `gin-index-for-arrays.rra` → `inverted-index-for-arrays.rra`
2. Refactor `gin-index-for-jsonb.rra` → `inverted-index-for-jsonb.rra`
3. Refactor `rum-index-for-fulltext.rra` → `inverted-index-for-fulltext.rra`
4. Update cost models to use `IndexMetadata.estimate_scan_cost()`

### Phase 3: DocumentDB integration (1-2 weeks)

1. Add DocumentDB RUM detection
2. Map BSON operator families to capabilities
3. Test with DocumentDB-specific queries

### Phase 4: Testing and documentation (1-2 weeks)

1. Parameterized tests with multiple index types
2. Update rule authoring guide
3. Add examples to documentation
4. Write migration guide for existing rules

## References

- PostgreSQL: `pg_index`, `pg_am`, `pg_opfamily` catalogs
- PostgreSQL GIN indexes: https://www.postgresql.org/docs/current/gin.html
- PostgreSQL RUM extension: https://github.com/postgrespro/rum
- DocumentDB extended RUM: pg_documentdb source code
- Apache Calcite index metadata: https://calcite.apache.org/
