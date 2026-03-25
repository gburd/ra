# RFC 0062: DocumentDB / MongoDB Query Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Implemented
- Tracking Issue: TBD

## Summary

Ra should detect and optimize queries originating from Microsoft's documentdb
PostgreSQL extension, which provides MongoDB compatibility by translating
MongoDB wire protocol operations into PostgreSQL queries over BSON-typed
columns. DocumentDB stores all documents as BSON in PostgreSQL, uses GIN
indexes for query acceleration, and translates MongoDB operators ($eq, $gt,
$in, $regex, $elemMatch, etc.) into custom PostgreSQL operators. Ra can
improve query plans by understanding BSON query patterns, providing better
selectivity estimates, recommending appropriate GIN index configurations,
and applying document-specific rewrite rules.

## Motivation

DocumentDB is a growing PostgreSQL extension that makes PostgreSQL act as a
MongoDB-compatible document database. It powers Azure Cosmos DB for MongoDB
vCore and is now open source. Production deployments handle millions of
document operations per second.

Ra currently treats documentdb's BSON columns and custom operators as opaque.
This creates several optimization gaps:

**1. Poor selectivity estimation.** DocumentDB's core selectivity function
in `pg_documentdb_core/src/planner/selectivity.c` returns a fixed 0.01
(1%) for all BSON operators. The improved version in
`pg_documentdb/src/query/bson_dollar_selectivity.c` uses tiered heuristics
but lacks histogram-based estimation for most operators. This leads to
incorrect join ordering and scan strategy selection.

**2. Missing compound index recommendations.** DocumentDB supports compound
GIN indexes via `bson_gin_composite_core.c` that index multiple document
paths. Ra's index advisor has no awareness of BSON path patterns and cannot
recommend compound document indexes for common query shapes.

**3. Suboptimal aggregation pipeline execution.** DocumentDB translates
MongoDB aggregation pipelines ($match, $group, $project, $unwind, $lookup)
into PostgreSQL queries via `ExpandAggregationFunction` in
`documents_planner.c`. The resulting SQL may have suboptimal join orders
or missing predicate pushdown opportunities.

**4. No schema inference.** MongoDB collections are schemaless, but most
collections have a dominant schema pattern. Ra could infer schema from
sampled documents to improve cost estimation and suggest typed indexes.

**Expected optimization impact:**

| Query Pattern | Current Plan | Optimized Plan | Gain |
|--------------|-------------|---------------|------|
| Equality on indexed path | GIN scan, 1% selectivity | GIN scan, statistics-based selectivity | 2-10x |
| $in with large array | Sequential scan | GIN scan with batched lookup | 10-100x |
| $regex on text field | Sequential scan | GIN trgm-aware scan | 10-100x |
| Aggregation with $match + $group | Hash aggregate on full scan | Index scan + streaming aggregate | 5-50x |
| $lookup (cross-collection join) | Nested loop, no index | Hash join with pushed filters | 5-20x |

## Guide-level explanation

### DocumentDB detection

When Ra's planner hook detects documentdb is installed (via `pg_extension`),
it activates document-aware optimization rules. Detection includes checking
for both `pg_documentdb_core` and `pg_documentdb` extensions:

```sql
SELECT extname, extversion
FROM pg_extension
WHERE extname IN ('documentdb_core', 'documentdb');
```

### BSON query pattern recognition

Ra recognizes documentdb's translated query patterns. A MongoDB query like:

```javascript
db.users.find({ age: { $gt: 25 }, status: "active" })
```

Becomes a PostgreSQL query using documentdb's custom operators:

```sql
SELECT document FROM documentdb_api.collection('mydb', 'users')
WHERE document @> '{"age": {"$gt": 25}}'
  AND document @= '{"status": "active"}';
```

Ra detects the `@>` and `@=` operators as documentdb BSON comparison
operators and applies document-aware selectivity estimation:

1. For `@=` (equality): check if a GIN index exists on the path, estimate
   selectivity from index statistics or use 0.001-0.01 based on
   distinct value count.
2. For `@>` (range comparison): use histogram-based estimation if available,
   or apply range selectivity heuristic (0.33 for unbounded, 0.11 for
   bounded ranges).
3. For combined predicates: multiply selectivities with independence
   assumption, applying a correlation damping factor for paths that are
   likely correlated.

### Compound index recommendation

When Ra sees repeated queries filtering on multiple BSON paths, it
recommends a compound GIN index:

```sql
-- For queries filtering on {status, age, name}:
CREATE INDEX idx_users_compound
  ON documentdb_data.documents_NNN
  USING GIN (document bson_gin_composite_ops)
  WITH (paths = '{"status": 1, "age": 1, "name": 1}');
```

### Aggregation pipeline optimization

For MongoDB aggregation pipelines that documentdb translates to SQL, Ra
applies standard relational optimizations on the translated query:

```javascript
// MongoDB:
db.orders.aggregate([
  { $match: { status: "completed", date: { $gte: ISODate("2026-01-01") } } },
  { $group: { _id: "$customer_id", total: { $sum: "$amount" } } },
  { $sort: { total: -1 } },
  { $limit: 10 }
])
```

Ra optimizations on the translated SQL:
1. Push $match predicates below $group as WHERE clauses
2. Recommend index on (status, date) paths for the $match stage
3. Apply top-N sort optimization (RFC 0031) for $sort + $limit
4. Use streaming aggregate if input is ordered by customer_id

## Reference-level explanation

### DocumentDB operator mapping

DocumentDB defines custom PostgreSQL operators for BSON queries. Ra must
recognize these operators to apply appropriate optimization rules:

| MongoDB Operator | PG Operator | Index Strategy | Selectivity Class |
|-----------------|------------|---------------|-------------------|
| `$eq` | `@=` | GIN equality | Low (0.001-0.01) |
| `$gt`, `$gte` | `@>`, `@>=` | GIN range | Medium (0.33) |
| `$lt`, `$lte` | `@<`, `@<=` | GIN range | Medium (0.33) |
| `$ne` | negation of `@=` | No index | High (0.99) |
| `$in` | `@*=` | GIN multi-equality | Low * N elements |
| `$nin` | `@!*=` | No index | High (1 - low*N) |
| `$all` | `@&=` | GIN intersection | Low^N |
| `$regex` | `@~` | GIN prefix | Medium (0.1-0.5) |
| `$exists` | exists check | GIN term | High (0.5-0.99) |
| `$elemMatch` | composite | GIN nested | Low (0.01) |
| `$geoWithin` | `@\|-\|` | GiST spatial | Depends on area |
| `$geoIntersects` | `@\|#\|` | GiST spatial | Depends on geometry |
| `$near` | distance | GiST KNN | N/A (ordering) |

### BSON GIN index cost model

DocumentDB's GIN indexes store entries as `{path}{typeCode}{value}` terms.
The cost model for GIN scans on BSON documents:

```
gin_bson_scan_cost =
    n_terms * term_lookup_cost        -- posting list lookups
  + n_matching_docs * recheck_cost    -- BSON validation
  + n_matching_docs * heap_fetch_cost -- tuple retrieval
```

Where:
- `term_lookup_cost = 3.0` (GIN posting list traversal)
- `recheck_cost = 2.0` (BSON deserialization + predicate re-evaluation)
- `heap_fetch_cost = 1.5` (random I/O for document retrieval)

For compound GIN indexes, the cost is lower because fewer posting lists
are intersected:

```
compound_gin_cost =
    n_paths * term_lookup_cost
  + bitmap_intersection_cost          -- posting list AND
  + n_matching_docs * recheck_cost
```

### Schema inference for cost estimation

Ra can infer document schema by sampling documents from
`documentdb_data.documents_NNN` tables:

```sql
-- Sample 1000 documents to infer schema
SELECT bson_get_value(document, 'fieldName') IS NOT NULL AS has_field,
       pg_typeof(bson_get_value(document, 'fieldName')) AS field_type
FROM documentdb_data.documents_NNN
TABLESAMPLE BERNOULLI(1)
LIMIT 1000;
```

Schema inference provides:
- Field existence probability (for $exists selectivity)
- Value type distribution (for $type selectivity)
- Distinct value estimate (for $eq selectivity)
- Array length distribution (for $elemMatch, $size selectivity)

### Aggregation pipeline rewrite rules

Rule 1: **$match pushdown.** When $match appears after $project or $unwind,
check if the filter references only fields that exist before the
transformation. If so, push $match earlier in the pipeline.

Rule 2: **$lookup optimization.** MongoDB's $lookup (equivalent to LEFT JOIN)
translates to a subquery or lateral join. Ra can convert this to a hash
join when the "from" collection is large and the "localField" has a
matching index.

Rule 3: **$group + $sort + $limit as top-N.** When $group is followed by
$sort + $limit, Ra applies top-N aggregation to avoid materializing all
groups.

Rule 4: **$unwind + $group folding.** When $unwind expands an array field
and $group immediately aggregates it back, Ra can eliminate the
expand-collapse cycle and apply the aggregation directly on array elements.

### Index recommendation rules

Rule 1: **Single-field index for equality.** When queries use `@=` on a
specific BSON path without a matching GIN index:

```
IF query uses @= on path P
   AND no GIN index covers path P
   AND query frequency > threshold
THEN recommend:
  SELECT documentdb_api_internal.create_indexes_non_concurrently(
    'dbname',
    '{"createIndexes": "collection", "indexes": [
      {"key": {"P": 1}, "name": "idx_P"}
    ]}'::bson
  );
```

Rule 2: **Compound index for multi-path queries.** When queries
consistently filter on the same set of paths:

```
IF query uses operators on paths {P1, P2, ..., Pn}
   AND n >= 2
   AND no compound index covers {P1, ..., Pn}
THEN recommend compound index with most selective path first
```

Rule 3: **Text index for $regex.** When queries use `$regex` or `$text`
operators, recommend documentdb's text index (backed by RUM):

```
IF query uses $regex or $text on path P
   AND no text index exists on P
THEN recommend text index on P
```

### Error handling

All documentdb-specific optimizations are non-fatal. When BSON parsing,
schema inference, or index metadata queries fail, Ra falls back to treating
the query as a standard PostgreSQL query with opaque operators.

```rust
#[derive(Debug, thiserror::Error)]
pub enum DocumentDbError {
    #[error(
        "BSON path extraction failed for {path}: {reason}; \
         using default selectivity"
    )]
    PathExtractionFailed { path: String, reason: String },

    #[error(
        "Schema inference failed for collection {collection}: \
         {reason}; skipping schema-based optimization"
    )]
    SchemaInferenceFailed {
        collection: String,
        reason: String,
    },

    #[error(
        "DocumentDB version {version} not supported for \
         {feature}; minimum required: {minimum}"
    )]
    UnsupportedVersion {
        version: String,
        feature: String,
        minimum: String,
    },
}
```

## Drawbacks

**BSON parsing overhead.** Extracting path information from BSON query
operators requires parsing the operator arguments. This adds CPU cost
to the planning phase. Mitigation: cache parsed operator metadata per
query.

**Schema inference cost.** Sampling documents for schema inference adds
I/O during the first planning call for a collection. Mitigation: cache
schema per collection, refresh on significant writes.

**DocumentDB version coupling.** DocumentDB is under active development
and may change its internal operator representations, GIN index format,
or planner hooks. Ra must handle version differences gracefully.

**Limited statistics.** DocumentDB's selectivity estimation is basic
(tiered heuristics). Ra's improvements depend on access to GIN index
statistics that PostgreSQL does not expose directly for custom operator
classes.

## Rationale and alternatives

### Why optimize for documentdb specifically

DocumentDB represents a growing use case where PostgreSQL serves as a
MongoDB-compatible document database. The query patterns are distinct
from standard SQL:
- All data is in BSON columns (no normalized schema)
- Queries use custom operators, not standard SQL predicates
- Indexes are GIN-based with custom operator classes
- Aggregation pipelines translate to complex subqueries

These patterns require specific optimization rules that do not apply
to standard PostgreSQL queries.

### Alternative: rely on documentdb's own planner

DocumentDB has its own planner hooks (`documents_planner.c`,
`documents_custom_planner.c`) that handle index selection and query
transformation. However, documentdb's planner:
- Uses basic selectivity estimation (fixed heuristics)
- Does not optimize the SQL generated from aggregation pipelines
- Does not provide cross-collection query optimization
- Does not recommend indexes based on workload analysis

Ra adds value by optimizing the translated SQL and providing
workload-aware index recommendations.

### Alternative: improve documentdb's planner directly

Contributing improvements to documentdb's planner is complementary to
Ra's approach but serves a different purpose. Ra optimizes from the
outside (analyzing the generated SQL), while documentdb optimizes from
the inside (generating better SQL). Both approaches can coexist.

## Prior art

### MongoDB query optimizer

MongoDB's own query optimizer uses:
- Plan cache with shape-based lookup (similar to Ra's genetic
  fingerprinting, RFC 0060)
- Multi-plan execution with adaptive selection
- Index intersection for queries matching multiple indexes
- Covered queries that return results entirely from indexes

Ra can apply similar strategies to the PostgreSQL-translated versions
of these queries.

### Apache Calcite MongoDB adapter

Calcite's MongoDB adapter translates MongoDB queries to relational
algebra and applies standard optimization rules. This is conceptually
similar to what Ra would do: treat the translated SQL as a relational
expression and optimize it.

### FerretDB

FerretDB is another MongoDB-compatible layer on PostgreSQL that stores
documents as JSONB (not BSON). Its approach is simpler but faces the
same optimization challenges. Lessons from optimizing documentdb queries
apply to FerretDB as well.

## Unresolved questions

1. **GIN statistics access.** Can Ra access GIN index statistics (term
   frequencies, posting list sizes) through PostgreSQL's catalog, or are
   these internal to the GIN AM?

2. **Custom scan interaction.** DocumentDB uses custom scan nodes for
   text and vector search. How should Ra interact with these custom scans
   -- should it optimize the inner plan, or treat the custom scan as
   opaque?

3. **Schema inference trigger.** When should schema inference run -- on
   first query, periodically, or triggered by ANALYZE?

4. **Aggregation pipeline boundaries.** DocumentDB translates entire
   pipelines to SQL. Can Ra intercept at the pipeline stage level, or
   only optimize the final SQL?

## Future possibilities

### Document schema evolution tracking

Track schema changes over time to detect when queries become
less efficient due to schema drift (e.g., a field that was always
present starts appearing in only 50% of documents).

### Cross-collection join optimization

When $lookup spans collections, Ra can analyze the join pattern and
recommend co-located storage or materialized views.

### BSON-to-relational migration advisor

For workloads that would benefit from normalized schema, Ra could
recommend migrating specific document patterns to relational tables
with foreign keys.

## Implementation

**Status**: Implemented in Ra v0.1.0

**Files:**
- `crates/ra-engine/src/documentdb_optimizer.rs` (1393 lines, 26 tests)

**Key Features:**
- BSON operator recognition (`@=`, `@>`, `@<`, `@*=`, `@~`, etc.)
- Operator-specific selectivity estimation (per-operator defaults and GIN-aware estimates)
- GIN index scan cost modeling with compound index support
- GIN index recommendation engine for multi-path queries
- 5 e-graph rewrite rules for DocumentDB query patterns
- $match pushdown below aggregation
- Error handling with graceful fallback to standard PostgreSQL behavior

**Tests:**
- 26 unit tests covering operator parsing, selectivity, cost modeling, rewrite rules, and error messages
- Tests in `crates/ra-engine/src/documentdb_optimizer.rs` (inline `#[cfg(test)]` module)

**Commit:** `f685f077` (feat: Implement RFC 0062)
