# RFC 0080: DocumentDB RUM Fork for BSON-Aware Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Extend Ra's DocumentDB optimizer (RFC 0062) with BSON-aware RUM index
optimization. DocumentDB ships its own `pg_documentdb_extended_rum` fork
that provides ordered scans, boundary-qualified retrieval, and four
BSON-specific operator families for single-path, composite-path, hashed,
and shard-unique indexing. Ra should detect when DocumentDB's extended
RUM access method is installed, map MongoDB query operators ($text,
$near, array containment) to the appropriate RUM operator family, and
apply a BSON-tuned cost model that accounts for the wider posting entries
and ordered scan capabilities that distinguish RUM from GIN.

## Motivation

RFC 0062 implemented GIN-based optimization for DocumentDB's BSON queries.
However, DocumentDB does not rely solely on GIN. Its
`pg_documentdb_extended_rum` extension provides a custom RUM access method
that wraps eight index AM callbacks (ambeginscan, amgettuple, amgetbitmap,
amrescan, amendscan, ambuild, aminsert, amcostestimate) with
BSON-specific logic. This RUM fork is the backbone for:

**1. Full-text search ($text).** DocumentDB translates MongoDB `$text`
queries into PostgreSQL tsvector operations executed against RUM indexes.
RUM's in-index phrase position verification and distance-ordered scans
make `$text` queries substantially faster than the GIN equivalent
(no heap recheck, no external sort for ranked results).

**2. Geospatial distance ordering ($near, $nearSphere).** The `|-<>`
distance operator registered in the composite-path operator family
provides ordered geospatial retrieval directly from the index, which
GIN cannot provide at all.

**3. Array containment with ordering.** Standard GIN can verify array
containment ($all, $elemMatch) but cannot order results. RUM's extended
posting entries carry additional metadata that enables ordered array
scans.

**4. Compound path queries.** The four operator families
(`bson_extended_rum_single_path_ops`, `bson_extended_rum_composite_path_ops`,
`documentdb_extended_rum_hashed_ops`, `bson_extended_rum_unique_shard_path_ops`)
cover distinct indexing strategies. Ra must select the right operator
family and estimate costs accordingly.

**Expected optimization impact:**

| Query Pattern | GIN Plan | RUM Plan | Gain |
|--------------|----------|----------|------|
| $text with $sort by score | GIN scan + heap recheck + sort | RUM distance scan | 10-50x |
| $near with $limit | Seq scan + sort | RUM KNN ordered scan | 50-200x |
| $elemMatch + $sort | GIN bitmap scan + sort | RUM ordered scan | 5-20x |
| $all on array field | GIN posting intersection | RUM with ordering metadata | 2-5x |
| Compound $eq + $text | Two GIN scans + bitmap AND | Single RUM composite scan | 3-10x |

## Guide-level explanation

### DocumentDB RUM detection

When Ra detects the `documentdb_extended_rum` extension alongside
`documentdb_core`, it activates RUM-aware optimization rules:

```sql
SELECT extname, extversion
FROM pg_extension
WHERE extname IN ('documentdb_core', 'documentdb',
                  'documentdb_extended_rum');
```

### BSON operator to RUM mapping

MongoDB query patterns that DocumentDB translates to PostgreSQL are
further classified by which RUM operator family can serve them:

```
$text search  -> bson_extended_rum_single_path_ops   (FTS with ordering)
$near query   -> bson_extended_rum_composite_path_ops (distance |-<>)
$eq + $text   -> bson_extended_rum_composite_path_ops (compound scan)
$all on array -> bson_extended_rum_single_path_ops    (array containment)
$eq (hashed)  -> documentdb_extended_rum_hashed_ops   (equality lookup)
unique shard  -> bson_extended_rum_unique_shard_path_ops (unique constraint)
```

### Cost model differences from GIN

RUM posting entries are wider than GIN because they include positional
data and optional addon fields. The cost model accounts for:

- Higher per-entry I/O cost (wider postings)
- Lower total cost for ordered queries (no external sort)
- Boundary-qualified scans that skip irrelevant posting list segments
- In-index phrase verification (no heap recheck for $text)

### Recommendation engine

When Ra sees queries that would benefit from RUM over GIN, it recommends
switching the index type:

```sql
-- For $text queries on a collection:
SELECT documentdb_api_internal.create_indexes_non_concurrently(
  'mydb',
  '{"createIndexes": "articles",
    "indexes": [{"key": {"content": "text"},
                 "name": "idx_content_text"}]}'::bson
);
-- DocumentDB internally creates a RUM index for text indexes
```

## Reference-level explanation

### DocumentDB extended RUM operator families

| Operator Family | Strategy | Use Case |
|----------------|----------|----------|
| `bson_extended_rum_single_path_ops` | Single JSON path with ordering | $text, $regex, single-field queries |
| `bson_extended_rum_composite_path_ops` | Multiple paths + distance (|-<>) | $near, compound queries, $text + $sort |
| `documentdb_extended_rum_hashed_ops` | Hash-based equality | High-cardinality $eq, _id lookups |
| `bson_extended_rum_unique_shard_path_ops` | Unique constraint enforcement | Shard key uniqueness |

### RUM BSON cost model

```
rum_bson_scan_cost =
    n_terms * term_lookup_cost        -- posting list lookups (wider entries)
  + boundary_cost                     -- boundary qualification overhead
  + n_matching * distance_cost        -- distance computation (if ordered)
  + n_matching * heap_fetch_cost      -- tuple retrieval

Where:
  term_lookup_cost = 3.5   (vs GIN's 3.0: wider posting entries)
  boundary_cost    = 1.0   (boundary qualifier evaluation)
  distance_cost    = 0.3   (per-result distance computation)
  heap_fetch_cost  = 1.5   (same as GIN)
```

For ordered queries with LIMIT k:
```
rum_bson_ordered_cost =
    n_terms * term_lookup_cost
  + k * 1.2 * (distance_cost + heap_fetch_cost)  -- overfetch 20%
```

### BSON operator RUM classification

| MongoDB Op | PG Operator | RUM Opfamily | Supports Ordering | Cost Class |
|-----------|------------|-------------|------------------|-----------|
| $text | @@ (tsvector) | single_path | Yes (distance) | Low with LIMIT |
| $near | |-<> (distance) | composite_path | Yes (KNN) | Low with LIMIT |
| $nearSphere | |-<> (distance) | composite_path | Yes (KNN) | Low with LIMIT |
| $regex | @~ | single_path | No | Medium |
| $all | @&= | single_path | With addon | Low |
| $elemMatch | composite | single_path | With addon | Low |
| $eq | @= | hashed_ops | No | Very low |
| $in | @*= | single_path | No | Medium |

### Rewrite rules

New e-graph rewrite rules for DocumentDB RUM optimization:

1. **GIN-to-RUM upgrade**: When a BSON filter uses an operator that
   benefits from RUM ordering, annotate the scan node to prefer the
   RUM access method.

2. **Sort elimination for $text**: When `$text` results are sorted
   by score and a RUM index exists, eliminate the sort node since
   RUM provides ordered retrieval.

3. **$near KNN pushdown**: When `$near` with `$limit` appears above
   a scan, push the limit into the RUM KNN scan to avoid fetching
   all matches.

4. **Compound RUM scan**: When multiple BSON predicates filter the
   same collection and a composite RUM index covers all paths,
   merge into a single compound RUM scan.

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum DocumentDbRumError {
    #[error(
        "DocumentDB extended RUM not installed; \
         using GIN cost model instead"
    )]
    RumNotInstalled,

    #[error(
        "BSON operator '{operator}' not mappable to RUM opfamily; \
         falling back to GIN scan"
    )]
    OperatorNotMappable { operator: String },

    #[error(
        "RUM index on collection '{collection}' uses unknown \
         opfamily '{opfamily}'; skipping RUM optimization"
    )]
    UnknownOpfamily {
        collection: String,
        opfamily: String,
    },
}
```

## Drawbacks

**Wider posting entries.** RUM indexes are 20-40% larger than equivalent
GIN indexes. For pure equality queries without ordering needs, GIN
remains more space-efficient.

**Build cost.** RUM index builds are approximately 1.4x slower than GIN.
For write-heavy workloads, this is a meaningful overhead.

**DocumentDB version coupling.** The `pg_documentdb_extended_rum` fork
evolves independently from upstream RUM. Operator families and strategies
may change between DocumentDB releases.

**Limited to DocumentDB RUM fork.** Standard PostgreSQL RUM (from
postgrespro/rum) has different operator classes. This optimization is
specific to DocumentDB's fork.

## Rationale and alternatives

### Why extend the existing documentdb_optimizer

The DocumentDB optimizer (RFC 0062) already handles BSON operator
recognition, selectivity estimation, and GIN cost modeling. Adding RUM
awareness as an extension to the same module maintains a single code
path for DocumentDB query optimization and avoids duplicating operator
parsing logic.

### Alternative: separate rum_documentdb module

A separate module would provide cleaner separation but would duplicate
operator parsing and selectivity estimation. The cost of maintaining
two copies of BSON operator logic outweighs the modularity benefit.

### Alternative: rely on DocumentDB's own RUM cost estimation

DocumentDB's `rumselfuncs.c` provides cost estimation, but it uses
generic GIN-style cost formulas without BSON-specific knowledge. Ra
can provide better estimates by combining BSON selectivity data with
RUM cost parameters.

## Prior art

- **DocumentDB pg_documentdb_extended_rum**: The fork itself, which
  wraps RUM AM callbacks with BSON-aware logic. Four operator families
  cover single-path, composite, hashed, and shard-unique strategies.

- **PostgreSQL RUM (postgrespro/rum)**: The upstream RUM extension
  for PostgreSQL that provides distance-ordered FTS. DocumentDB's fork
  is derived from this but adds BSON-specific operator families.

- **MongoDB $text index**: MongoDB's native text search uses a dedicated
  text index type that supports scoring and sorting. DocumentDB maps
  this to RUM.

- **MongoDB $near geospatial**: MongoDB uses 2dsphere indexes for
  geospatial queries. DocumentDB maps $near to RUM distance operators.

## Unresolved questions

1. **RUM statistics access.** Can Ra access RUM-specific statistics
   (posting list widths, distance histograms) through DocumentDB's
   catalog extensions?

2. **Version detection.** How to detect which version of
   `pg_documentdb_extended_rum` is installed and which operator
   families are available?

3. **Compound index selection.** When both single-path and composite-path
   RUM indexes exist, how to choose between them for multi-predicate
   queries?

## Future possibilities

- **Hybrid RUM + GIN selection**: For mixed workloads, recommend RUM
  for ordered queries and GIN for pure boolean queries on the same
  collection.
- **RUM index advisor for DocumentDB**: Proactive recommendation of
  text indexes ($text) and 2dsphere indexes ($near) based on observed
  query patterns.
- **Cross-collection $lookup with RUM**: When $lookup joins two
  collections and the foreign collection has a RUM text index,
  optimize the join to use RUM's ordered scan.
