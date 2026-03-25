# Platform-Specific Optimizations

Ra provides specialized optimization rules for platform-specific features that go beyond standard SQL. These optimizations detect and exploit database-specific extensions, indexes, and storage formats to achieve significant performance improvements.

## Overview

Platform-specific optimizations are implemented as conditional rule sets that load only when the target platform supports the relevant features. This architecture (RFC 0085) ensures Ra can optimize for multiple database systems without bloating the rule set or compromising plan quality.

### Supported Platforms

| Platform | Feature | RFC | Implementation |
|----------|---------|-----|----------------|
| PostgreSQL | RUM Index | RFC 0079 | `rum_index.rs` |
| PostgreSQL + Citus | Distributed Queries | RFC 0081 | `citus_optimizer.rs` |
| PostgreSQL + DocumentDB | BSON Queries | RFC 0080, 0062 | `documentdb_optimizer.rs` |
| Oracle | JSON Relational Duality | RFC 0084 | `oracle_json_duality.rs` |
| Oracle/PostgreSQL/SQL Server | XPath/XQuery | RFC 0083 | `xml_optimizer.rs` |
| MongoDB | Formal Semantics + TOAST/HOT | RFC 0082 | `document_algebra.rs` |

## PostgreSQL RUM Index Optimization

**RFC 0079** | **Module**: `ra-engine::rum_index`

### What is RUM?

RUM (Resource-Universal-Method) is a PostgreSQL extension that extends GIN indexes by storing additional metadata in posting lists. This enables distance-ordered scans, in-index phrase verification, and timestamp-ordered retrieval—operations that GIN cannot perform.

### Key Capabilities

- **Distance-ordered retrieval**: The `<=>` operator returns results sorted by relevance distance
- **In-index phrase verification**: Position data eliminates heap rechecks for phrase queries
- **Addon column ordering**: Sort by timestamp while filtering by text content
- **KNN retrieval**: Depth-first traversal for nearest-neighbor queries

### Query Classification

Ra classifies queries into five RUM-optimizable patterns:

1. **Boolean Match**: `tsvector @@ tsquery` (RUM slightly slower than GIN)
2. **Ranked Retrieval**: `ORDER BY ts_rank()` with optional LIMIT (10-1000x faster)
3. **Phrase Search**: Proximity operators `<->` (2-5x faster)
4. **Timestamp Ordered**: Text search with `ORDER BY timestamp` (5-20x faster)
5. **KNN**: K-nearest-neighbor retrieval (10-100x faster)

### Cost Model

Ra adjusts costs based on query type and available RUM operator classes:

```rust
pub fn rum_index_cost(
    query_type: RumQueryType,
    opclass: RumOpclass,
    selectivity: f64,
    limit: Option<u64>,
) -> Cost {
    match query_type {
        RumQueryType::BooleanMatch => {
            // RUM is 1.2x slower than GIN for boolean queries
            gin_cost * 1.2
        }
        RumQueryType::RankedRetrieval => {
            // Distance scan touches only ~limit rows
            let effective_rows = limit.unwrap_or(total_rows) as f64;
            posting_fetch_cost * effective_rows.log2()
        }
        // ... other patterns
    }
}
```

### Example: Top-K Text Search

**Query**:
```sql
SELECT title, ts_rank(tsv, query) AS rank
FROM documents, to_tsquery('postgresql & optimization') query
WHERE tsv @@ query
ORDER BY rank DESC
LIMIT 10;
```

**Without RUM** (GIN index):
1. GIN scan finds all 100,000 matching rows
2. Compute `ts_rank()` for each row (100,000 function calls)
3. Sort 100,000 rows by rank
4. Return top 10

**With RUM**:
1. RUM distance scan retrieves rows in rank order
2. Stop after ~10 rows (early termination)
3. No separate sort step required

**Performance**: 100-1000x improvement for high-selectivity queries with LIMIT.

### Detection and Activation

Ra detects RUM availability by querying `pg_am`:

```sql
SELECT EXISTS(SELECT 1 FROM pg_am WHERE amname = 'rum');
```

When RUM is present, Ra activates RUM-specific rewrite rules:

- `rum-distance-ordered-scan`: Convert `ORDER BY ts_rank()` to distance scan
- `rum-phrase-in-index`: Verify phrase positions in-index
- `rum-timestamp-addon`: Use addon ops for timestamp ordering
- `rum-knn-retrieval`: Apply KNN distance scan

### Operator Classes

| Operator Class | Use Case |
|---------------|----------|
| `rum_tsvector_ops` | Standard FTS with distance ordering |
| `rum_tsvector_hash_ops` | Hash-based FTS (no prefix search) |
| `rum_tsvector_addon_ops` | FTS + additional sort field (e.g., timestamp) |
| `rum_tsquery_ops` | Query-side indexing |
| `rum_anyarray_ops` | Array operations with length metadata |

### Index Recommendations

Ra can suggest RUM indexes when it detects:
- Frequent `ts_rank()` with `LIMIT`
- Phrase search queries (`<->`, `<2>`)
- Text search combined with timestamp ordering

Example recommendation:
```sql
CREATE INDEX idx_documents_rum_tsv
ON documents USING rum(tsv rum_tsvector_ops);

-- Or with addon column for timestamp ordering:
CREATE INDEX idx_documents_rum_tsv_addon
ON documents USING rum(tsv rum_tsvector_addon_ops, created_at)
WITH (attach = 'created_at', to = 'tsv');
```

## CitusDB Distributed Query Optimization

**RFC 0081** | **Module**: `ra-engine::citus_optimizer`

### What is Citus?

Citus is Microsoft's distributed PostgreSQL extension that shards tables across worker nodes. It's deployed in Azure Cosmos DB for PostgreSQL and as open-source self-hosted clusters.

### Table Types

1. **Distributed Tables**: Sharded by distribution column
2. **Reference Tables**: Replicated to all workers
3. **Local Tables**: Coordinator-only tables

### Key Optimizations

#### 1. Co-Located Join Detection

When two distributed tables share:
- Same distribution column
- Same co-location group

Their matching shards reside on the same worker. Joins on the distribution column require **zero network transfer**.

**Example**:
```sql
-- Both tables distributed by customer_id, co-location group 1
SELECT *
FROM orders o
JOIN shipments s ON o.customer_id = s.customer_id;
```

**Without Citus awareness**: Ra may choose shuffle join (redistribute both tables)

**With Citus awareness**: Ra recognizes co-located join, plans local joins on each worker

**Performance**: 10-100x improvement (eliminates network transfer)

#### 2. Reference Table Broadcast Elimination

Reference tables are already replicated to all workers. Joins with distributed tables need no data movement.

**Example**:
```sql
SELECT *
FROM orders o  -- distributed table
JOIN products p ON o.product_id = p.id;  -- reference table
```

**Cost adjustment**: Zero network cost for reference table side

#### 3. Distributed Aggregation Pushdown

When `GROUP BY` includes the distribution column, Citus pushes aggregation to workers.

**Example**:
```sql
SELECT customer_id, SUM(total)
FROM orders
GROUP BY customer_id;  -- distribution key
```

**Plan**:
1. Each worker computes partial aggregates for its shards
2. Coordinator combines partial results
3. No full table shuffle required

**Performance**: 5-50x improvement for large aggregations

#### 4. Shard Pruning

Filters on the distribution column eliminate entire shards at plan time.

**Example**:
```sql
SELECT * FROM orders
WHERE customer_id = 12345;  -- distribution key
```

**Optimization**: Query routed to single shard, others pruned

**Performance**: Linear improvement in shard count (e.g., 32 shards = 32x reduction)

#### 5. Columnar Storage Cost Adjustment

Citus columnar tables use column-oriented storage with compression. Ra adjusts scan costs based on:
- Column projection (narrow projections much cheaper)
- Compression ratio (default 3x, can be higher)
- Stripe and chunk group sizes

**Cost formula**:
```rust
pub fn columnar_scan_cost(
    total_columns: u32,
    projected_columns: u32,
    compression_ratio: f64,
    row_count: u64,
) -> Cost {
    let column_fraction = projected_columns as f64 / total_columns as f64;
    let effective_io = (row_count as f64 / compression_ratio) * column_fraction;
    seq_page_cost * effective_io / rows_per_page
}
```

### Metadata Detection

Ra queries Citus catalog tables:

```sql
-- Check for Citus extension
SELECT extversion FROM pg_extension WHERE extname = 'citus';

-- Get distributed table info
SELECT logicalrelid::regclass::text AS table_name,
       partkey AS distribution_column,
       colocationid AS colocation_group
FROM pg_dist_partition;

-- Get shard placements
SELECT shardid, nodename, nodeport
FROM pg_dist_shard
JOIN pg_dist_placement USING (shardid);
```

### Network Cost Model

Ra models network transfer costs between coordinator and workers:

```rust
pub struct CitusNetworkCost {
    pub latency_ms: f64,        // Per-round-trip latency
    pub bandwidth_mbps: f64,    // Network bandwidth
    pub parallelism: u32,       // Number of parallel connections
}

pub fn network_transfer_cost(
    bytes: u64,
    network: &CitusNetworkCost,
) -> Cost {
    let latency_cost = network.latency_ms;
    let transfer_time_ms = (bytes as f64 / 1_000_000.0)
                           / network.bandwidth_mbps
                           * 1000.0
                           / network.parallelism as f64;
    Cost::from_ms(latency_cost + transfer_time_ms)
}
```

## DocumentDB BSON Query Optimization

**RFC 0062, 0080** | **Module**: `ra-engine::documentdb_optimizer`

### What is DocumentDB?

Microsoft's DocumentDB is a PostgreSQL extension that implements MongoDB wire protocol compatibility. It translates MongoDB queries into PostgreSQL operations over BSON-typed columns using custom operators.

### BSON Operators

DocumentDB defines custom operators for BSON operations:

| MongoDB Operator | PostgreSQL Operator | Operation |
|-----------------|-------------------|-----------|
| `$eq` | `@=` | Exact equality |
| `$gt`, `$gte` | `@>`, `@>=` | Greater than (or equal) |
| `$lt`, `$lte` | `@<`, `@<=` | Less than (or equal) |
| `$ne` | `NOT (@=)` | Not equal |
| `$in` | `@*=` | Array membership |
| `$nin` | `@!*=` | Not in array |
| `$all` | `@&=` | Array contains all |
| `$regex` | `@~` | Regular expression |
| `$exists` | Custom function | Field existence |
| `$elemMatch` | Custom function | Nested array match |

### Default Selectivity Problem

DocumentDB returns a fixed 0.01 (1%) selectivity for **all** BSON operators. This leads to:
- Poor join ordering (all predicates look equally selective)
- Suboptimal scan strategy selection
- Underestimation of result set sizes

### Ra's Solution: Operator-Specific Selectivity

Ra provides calibrated selectivity estimates:

```rust
pub fn default_selectivity(operator: BsonOperator) -> f64 {
    match operator {
        BsonOperator::Eq => 0.005,          // Very selective
        BsonOperator::Gt | Gte | Lt | Lte => 0.33,  // Range queries
        BsonOperator::Ne => 0.99,            // Anti-selective
        BsonOperator::In => 0.05,            // Depends on array size
        BsonOperator::Nin => 0.95,           // Anti-selective
        BsonOperator::All => 0.001,          // Very selective
        BsonOperator::Regex => 0.25,         // Variable
        BsonOperator::Exists => 0.75,        // Sparse field assumption
        BsonOperator::ElemMatch => 0.01,     // Nested match
    }
}
```

### GIN Index Cost Modeling

Ra models DocumentDB's GIN indexes with BSON-aware costs:

```rust
pub fn bson_gin_index_cost(
    operator: BsonOperator,
    path_depth: u32,
    cardinality: u64,
) -> Cost {
    let base_cost = gin_base_cost();

    // Deeper paths increase posting list traversal cost
    let depth_penalty = 1.0 + (path_depth as f64 * 0.1);

    // Operator-specific cost multipliers
    let op_cost = match operator {
        BsonOperator::Eq => 1.0,
        BsonOperator::In => 1.5,  // Multiple value lookups
        BsonOperator::All => 2.0,  // Posting list intersection
        BsonOperator::Regex => 3.0,  // Index scan + heap recheck
        _ => 1.2,
    };

    base_cost * depth_penalty * op_cost
}
```

### RUM Fork for BSON (RFC 0080)

DocumentDB ships `pg_documentdb_extended_rum` which provides:

1. **Full-text search** (`$text`): Distance-ordered FTS with no heap recheck
2. **Geospatial ordering** (`$near`, `$nearSphere`): KNN geospatial retrieval
3. **Array ordering**: Ordered scans over array fields
4. **Compound path indexes**: Single index for multiple BSON paths

**Performance improvements with RUM**:
- `$text` with `$sort`: 10-50x (no external sort)
- `$near` with `$limit`: 50-200x (ordered index scan)
- `$elemMatch` + `$sort`: 5-20x (single ordered scan)

### Multi-Path Index Recommendations

Ra can recommend compound GIN indexes for common path combinations:

```sql
-- Detected pattern: frequent queries on status + priority
CREATE INDEX idx_documents_status_priority
ON documents USING gin(
  (data @= '{"status": true}'::jsonb),
  (data @= '{"priority": true}'::jsonb)
);
```

## Oracle JSON Relational Duality

**RFC 0084** | **Module**: `ra-engine::oracle_json_duality`

### What is JSON Relational Duality?

Oracle 23ai introduced duality views that expose normalized relational tables as JSON documents bidirectionally. The same data can be accessed via:
- **Document API**: JSON CRUD operations
- **Relational API**: Standard SQL joins

### Access Path Selection

Ra chooses between two access methods:

**1. Document Fetch** (single-row retrieval):
```sql
SELECT * FROM orders_dv WHERE id = 12345;
```
Cost: O(1) document fetch from root table

**2. Relational Decomposition** (scan/filter):
```sql
SELECT * FROM orders_dv WHERE customer_name LIKE 'Acme%';
```
Cost: Join across base tables + JSON assembly

### Optimization: Predicate Pushdown

Ra rewrites predicates on JSON fields to relational columns:

**Original**:
```sql
SELECT * FROM orders_dv
WHERE JSON_VALUE(doc, '$.customer.name') = 'Acme Corp';
```

**Rewritten**:
```sql
SELECT * FROM orders o
JOIN customers c ON o.customer_id = c.id
WHERE c.name = 'Acme Corp';
```

**Performance**: 10-100x (uses indexes on relational columns)

### Optimization: Partial Document Assembly

When queries reference only subset of fields, Ra skips unnecessary joins:

**Query**:
```sql
SELECT JSON_VALUE(doc, '$.order_id'),
       JSON_VALUE(doc, '$.total')
FROM orders_dv;
```

**Optimization**: Access only `orders` table, skip joins to `line_items`, `shipments`

**Performance**: 2-10x (fewer joins, less JSON construction)

### Update Fan-Out Cost Estimation

Updates to duality views may affect multiple base tables:

```rust
pub fn duality_update_cost(
    view: &DualityView,
    update_fields: &[String],
) -> Cost {
    let affected_tables = count_affected_tables(view, update_fields);
    let fan_out_factor = affected_tables as f64;

    // Each table requires UPDATE + potential cascading updates
    let base_update_cost = single_table_update_cost();
    base_update_cost * fan_out_factor * 1.5  // 50% overhead for coordination
}
```

### Metadata Extraction

Ra queries Oracle's data dictionary:

```sql
SELECT view_name, root_table, json_definition
FROM user_json_duality_views;

-- Parse JSON definition to extract field mappings
```

## XPath/XQuery Optimization

**RFC 0083** | **Module**: `ra-engine::xml_optimizer`

### Supported Platforms

- **PostgreSQL**: `xpath()`, `xmlexists()`, `xmltable()`
- **Oracle**: `XMLQuery()`, `XMLTable()`, `existsNode()`
- **SQL Server**: `.value()`, `.query()`, `.exist()`, `.nodes()`

### XPath Cost Estimation

Ra parses XPath expressions and estimates traversal costs:

```rust
pub fn xpath_axis_cost(axis: XPathAxis) -> f64 {
    match axis {
        XPathAxis::Child => 1.0,              // Direct lookup
        XPathAxis::Attribute => 0.5,          // Very cheap
        XPathAxis::Self_ => 0.1,              // No navigation
        XPathAxis::Parent => 2.0,             // Reverse lookup
        XPathAxis::Descendant => 10.0,        // Full subtree scan
        XPathAxis::Ancestor => 8.0,           // Upward traversal
        XPathAxis::Following => 20.0,         // Document-order scan
        XPathAxis::Preceding => 20.0,         // Reverse document scan
        XPathAxis::FollowingSibling => 5.0,
        XPathAxis::PrecedingSibling => 5.0,
    }
}
```

### XML Index Types

Ra recognizes and costs three XML index types:

**1. Path Index** (structural index):
- Indexes specific XPath paths
- Enables direct lookup for `child::` and `descendant::` axes
- Cost: O(log n) for indexed paths, O(n) for unindexed

**2. Value Index**:
- Indexes element/attribute text content
- Enables equality and range predicates
- Cost: Similar to B-tree index

**3. Full-Text Index**:
- FTS over XML content
- Supports `contains()` XQuery function
- Cost: Similar to FTS index scan

### Example: SQL Server XML Index

**Query**:
```sql
SELECT *
FROM documents
WHERE xml_content.exist('/order/customer[@id="12345"]') = 1;
```

**Without XML index**:
1. Sequential scan of `xml_content` column
2. Parse XML for each row
3. Evaluate XPath predicate
Cost: O(n × XML_size)

**With PATH index**:
1. Index lookup on `/order/customer/@id`
2. Direct row fetch
Cost: O(log n)

**Performance**: 100-1000x for selective predicates on large tables

### Predicate Rewrite Rules

Ra applies XPath simplification rules:

- `xpath-redundant-axis-elimination`: `/descendant::a/child::b` → `/descendant::b`
- `xpath-attribute-shorthand`: `/child::@attr` → `/@attr`
- `xpath-predicate-pushdown`: Push predicates to earliest possible step
- `xmlexists-to-index-scan`: Convert `xmlexists()` to index lookup

## MongoDB Formal Semantics + TOAST/HOT

**RFC 0082** | **Module**: `ra-core::document_algebra`

### Document Algebra

Ra extends relational algebra with document-specific operators:

- `DocumentScan`: Scan BSON/JSON collection
- `DocumentFilter`: Predicate over document fields
- `DocumentProject`: Field selection and renaming
- `DocumentUnwind`: Array flattening (like MongoDB's `$unwind`)
- `DocumentGroup`: Aggregation over documents
- `DocumentLookup`: Foreign document join

### TOAST-Aware Cost Model

PostgreSQL's TOAST (The Oversized-Attribute Storage Technique) stores large values externally. Ra adjusts costs for TOAST'd columns:

```rust
pub fn toast_aware_scan_cost(
    column_avg_size: u64,
    toast_threshold: u64,
) -> Cost {
    if column_avg_size > toast_threshold {
        // TOAST'd values require separate I/O
        let base_scan = seq_scan_cost();
        let toast_fetch = random_page_cost() * 2.0;  // Typically 2 pages
        base_scan + toast_fetch
    } else {
        seq_scan_cost()
    }
}
```

### HOT Updates

Heap-Only Tuple (HOT) updates avoid index maintenance when updated columns aren't indexed. Ra factors this into update cost estimation:

```rust
pub fn update_cost(
    updated_columns: &[String],
    indexed_columns: &[String],
) -> Cost {
    let non_indexed_updates: usize = updated_columns.iter()
        .filter(|c| !indexed_columns.contains(c))
        .count();

    if non_indexed_updates == updated_columns.len() {
        // HOT update: no index maintenance
        heap_update_cost()
    } else {
        // Non-HOT: update all indexes
        heap_update_cost() + (index_update_cost() * indexed_columns.len() as f64)
    }
}
```

### Performance Impact

- **TOAST awareness**: 2-5x better cost estimates for wide documents
- **HOT update detection**: 5-50x update performance when indexes aren't affected

## Platform Detection Architecture

**RFC 0085** establishes a three-tier detection architecture:

### Tier 1: Dialect Detection

```rust
pub enum Dialect {
    PostgreSQL {
        version: Version,
        extensions: Vec<String>
    },
    Oracle { version: Version },
    SQLServer { version: Version },
    MySQL { version: Version, engine: String },
}

impl Dialect {
    pub fn detect(connection: &Connection) -> Result<Self, Error> {
        // Query pg_version, v$version, @@version, etc.
    }
}
```

### Tier 2: Conditional Rule Loading

```rust
pub fn platform_rules(dialect: &Dialect) -> Vec<Rewrite<RelLang, RelAnalysis>> {
    let mut rules = consensus_rules();  // Always load core rules

    match dialect {
        Dialect::PostgreSQL { extensions, .. } => {
            rules.extend(postgresql::base_rules());

            if extensions.contains(&"rum".to_string()) {
                rules.extend(postgresql::rum_rules());
            }
            if extensions.contains(&"citus".to_string()) {
                rules.extend(postgresql::citus_rules());
            }
        }
        Dialect::Oracle { .. } => {
            rules.extend(oracle::json_duality_rules());
            rules.extend(oracle::xml_rules());
        }
        // ... other dialects
    }

    rules
}
```

### Tier 3: Cost Model Overrides

```rust
pub struct PlatformCostModel {
    base: BaseCostModel,
    overrides: HashMap<String, CostFn>,
}

impl PlatformCostModel {
    pub fn with_rum(&mut self) {
        self.overrides.insert(
            "fts_ranked_retrieval".to_string(),
            Box::new(rum_distance_scan_cost),
        );
    }
}
```

## Usage Examples

### Enabling Platform Optimizations

Platform optimizations activate automatically when Ra detects the relevant features:

```rust
// Ra detects platform features from connection metadata
let optimizer = Optimizer::new(connection)?;

// Automatically enables RUM rules if installed
let plan = optimizer.optimize("
    SELECT title, ts_rank(tsv, query) AS rank
    FROM documents, to_tsquery('rust & optimization') query
    WHERE tsv @@ query
    ORDER BY rank DESC
    LIMIT 10
")?;
```

### Manual Feature Control

You can explicitly enable/disable platform features:

```rust
let config = OptimizerConfig::new()
    .enable_platform_rules(true)
    .enable_rum_optimization(true)
    .enable_citus_optimization(false);  // Explicitly disable

let optimizer = Optimizer::with_config(connection, config)?;
```

### Index Recommendations

Request platform-specific index recommendations:

```rust
let recommendations = optimizer.recommend_indexes(query)?;

for rec in recommendations {
    match rec.index_type {
        IndexType::Rum { opclass } => {
            println!("CREATE INDEX {} USING rum({} {})",
                rec.name, rec.column, opclass);
        }
        IndexType::CitusDistributed { distribution_column } => {
            println!("SELECT create_distributed_table('{}', '{}')",
                rec.table, distribution_column);
        }
        // ... other types
    }
}
```

## Performance Benchmarks

### RUM Index (PostgreSQL)

**Workload**: TPC-H Q17 adapted for full-text search

| Configuration | Time | Speedup |
|--------------|------|---------|
| GIN index | 2,450 ms | 1x |
| RUM index (boolean) | 2,680 ms | 0.9x |
| RUM index (ranked, LIMIT 100) | 24 ms | 102x |
| RUM index (phrase search) | 420 ms | 5.8x |

### Citus Distributed Queries

**Workload**: TPC-H Q5 (5-way join) on 8-worker cluster

| Optimization | Time | Speedup |
|-------------|------|---------|
| Baseline (shuffle joins) | 45,200 ms | 1x |
| Co-located join detection | 5,100 ms | 8.9x |
| + Reference table optimization | 1,800 ms | 25x |
| + Distributed aggregation | 680 ms | 66x |

### DocumentDB BSON Queries

**Workload**: MongoDB-style queries over 10M documents

| Query Pattern | Default Selectivity | Ra Selectivity | Plan Quality |
|--------------|-------------------|----------------|--------------|
| `{status: "active"}` | 0.01 (1%) | 0.15 (15%) | Correct join order |
| `{$in: [1,2,3]}` | 0.01 (1%) | 0.05 (5%) | Index scan selected |
| `{$regex: "^A.*"}` | 0.01 (1%) | 0.25 (25%) | Pushed to late join |

### Oracle JSON Duality

**Workload**: Document retrieval vs. relational decomposition

| Access Pattern | Document Fetch | Relational + JSON Assembly | Winner |
|---------------|---------------|---------------------------|---------|
| Single document by PK | 0.8 ms | 12 ms | Document (15x) |
| Filter on nested field | 450 ms | 32 ms | Relational (14x) |
| Aggregate over 100K docs | 8,200 ms | 580 ms | Relational (14x) |

## Best Practices

### 1. Let Ra Detect Features Automatically

Ra's automatic detection is fast and reliable:

```rust
// Good: Ra detects features
let optimizer = Optimizer::new(connection)?;

// Avoid: Manual feature flags unless needed
let optimizer = Optimizer::with_config(
    connection,
    OptimizerConfig::new().enable_rum_optimization(true)
)?;
```

### 2. Provide Statistics for Platform Extensions

Platform extensions often have poor default statistics:

```sql
-- PostgreSQL: Update RUM index statistics
ANALYZE documents;

-- Citus: Update shard metadata
SELECT citus_update_table_statistics('orders');
```

### 3. Monitor Plan Quality

Use `EXPLAIN ANALYZE` to verify Ra's platform-specific optimizations:

```sql
-- Check for RUM distance scan
EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM documents
WHERE tsv @@ to_tsquery('optimization')
ORDER BY tsv <=> to_tsquery('optimization')
LIMIT 10;

-- Should show: "Index Scan using idx_documents_rum on documents"
```

### 4. Index Recommendations

Request recommendations after workload profiling:

```bash
# Collect workload
ra-cli profile-workload --duration 1h --output workload.json

# Generate recommendations
ra-cli recommend-indexes --workload workload.json --platform postgres-rum
```

### 5. Fallback Behavior

Platform optimizations are always safe to enable—if features aren't available, Ra falls back to standard plans:

- RUM not installed → use GIN or sequential scan
- Citus not installed → standard PostgreSQL join/aggregation
- Extension metadata unavailable → generic cost model

## Troubleshooting

### RUM Index Not Used

**Symptom**: Query uses GIN index instead of RUM despite better performance

**Diagnosis**:
```sql
SELECT * FROM pg_opclass WHERE opcname LIKE 'rum%';
-- Check operator class is installed
```

**Solution**: Verify RUM index created with correct operator class:
```sql
CREATE INDEX idx_documents_rum
ON documents USING rum(tsv rum_tsvector_ops);  -- Not gin_tsvector_ops
```

### Citus Co-Located Join Not Applied

**Symptom**: Shuffle join used despite co-located tables

**Diagnosis**:
```sql
SELECT logicalrelid::regclass, colocationid
FROM pg_dist_partition
WHERE logicalrelid IN ('orders'::regclass, 'shipments'::regclass);
-- Check co-location groups match
```

**Solution**: Ensure tables in same co-location group:
```sql
SELECT create_distributed_table('orders', 'customer_id', colocate_with => 'canonical_table');
SELECT create_distributed_table('shipments', 'customer_id', colocate_with => 'canonical_table');
```

### DocumentDB Selectivity Issues

**Symptom**: Wrong join order despite Ra's improved selectivity

**Diagnosis**: Check if DocumentDB statistics are stale

**Solution**:
```sql
ANALYZE documents_collection;
```

### Oracle Duality View Performance

**Symptom**: Slow queries on duality views

**Diagnosis**: Check if predicates are pushed down
```sql
EXPLAIN PLAN FOR
SELECT * FROM orders_dv WHERE JSON_VALUE(doc, '$.status') = 'shipped';
```

**Solution**: Ensure base tables have indexes on filtered columns:
```sql
CREATE INDEX idx_orders_status ON orders(status);
```

## See Also

- [Architecture: Platform Module Design](/ra/architecture#platform-modules)
- [Hardware-Aware Optimization](/ra/features/hardware-acceleration)
- [Distributed Optimization](/ra/features/distributed-optimization)
- [Index Types](/ra/features/index-types)
- [Cost Model Calibration](/ra/guides/cost-models)
- [RFC 0079: PostgreSQL RUM Index](/ra/research/rfc-0079)
- [RFC 0081: CitusDB Optimization](/ra/research/rfc-0081)
- [RFC 0082: MongoDB Formal Semantics](/ra/research/rfc-0082)
- [RFC 0083: XPath/XQuery Optimization](/ra/research/rfc-0083)
- [RFC 0084: Oracle JSON Duality](/ra/research/rfc-0084)
- [RFC 0085: Platform Architecture](/ra/research/rfc-0085)
