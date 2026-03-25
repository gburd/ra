# PostgreSQL Extensions Analysis for Ra Optimizer

## Executive Summary

This report consolidates research findings from analyzing six PostgreSQL
extension categories for optimization opportunities in the Ra query
optimizer. The research covered documentdb (MongoDB compatibility),
PostGIS (spatial), TimescaleDB (time-series), pgvector (vector
similarity), full-text search (tsvector/pg_trgm), and advanced index
types (BRIN, GiST, SP-GiST, GIN, Bloom).

**Key finding**: PostgreSQL extensions introduce query patterns,
data types, and index access methods that the standard optimizer handles
suboptimally. Ra can provide 2-1000x improvement by understanding
extension-specific semantics and applying targeted optimization rules.

**RFCs generated**: 6 new RFCs (0062-0067), complementing the existing
RFC 0061 (PostgreSQL Extension-Aware Optimization).

## Findings by Extension

### 1. DocumentDB (MongoDB Compatibility) -- HIGH PRIORITY

**Architecture**: DocumentDB is Microsoft's open-source MongoDB
compatibility layer for PostgreSQL. It consists of three components:
- `pg_documentdb_core`: BSON datatype and operations
- `pg_documentdb`: Public API for CRUD, aggregation, indexing
- `pg_documentdb_gw`: MongoDB wire protocol gateway

**Query Translation Pipeline**:
1. MongoDB wire protocol -> gateway layer
2. Gateway translates to PostgreSQL function calls (find, aggregate, etc.)
3. Functions operate on BSON-typed columns using custom operators
4. GIN indexes accelerate BSON path lookups

**Key Operators Discovered**:
| MongoDB | PostgreSQL Operator | Index Strategy |
|---------|-------------------|----------------|
| `$eq` | `@=` | GIN equality |
| `$gt/$gte` | `@>`/`@>=` | GIN range |
| `$lt/$lte` | `@<`/`@<=` | GIN range |
| `$in` | `@*=` | GIN multi-equality |
| `$regex` | `@~` | GIN prefix |
| `$all` | `@&=` | GIN intersection |
| `$geoWithin` | `@\|-\|` | GiST spatial |
| `$near` | distance op | GiST KNN |

**Optimization Opportunities**:
- Selectivity estimation: Current implementation uses fixed 1% for
  all operators in core, slightly better tiered heuristics in the
  improved module. Statistics-based estimation available but underused.
- Compound GIN index recommendation for multi-path queries
- Aggregation pipeline optimization (predicate pushdown, join rewriting)
- Schema inference from document sampling
- Custom scan awareness (text and vector search use custom scan nodes)

**RFC**: 0062 (DocumentDB Query Optimization)

### 2. PostGIS (Spatial Queries)

**Architecture**: PostGIS adds geometry/geography types, spatial
operators (ST_Intersects, ST_DWithin, etc.), and spatial index support
(GiST R-tree, SP-GiST quad-tree, BRIN for sorted spatial data).

**Key Insight**: Spatial queries have a two-phase cost structure:
1. Bounding box pre-filter via GiST index (cheap)
2. Exact geometry test on filtered rows (expensive, varies 10-40x
   depending on geometry complexity)

PostgreSQL's planner applies flat cost to spatial functions, missing
the two-phase structure.

**Optimization Opportunities**:
- Two-phase cost model (bounding box + exact geometry)
- Geometry-type-aware cost multipliers (point vs polygon vs multi-polygon)
- SRID mismatch detection and cost accounting
- Spatial join reordering (minimize expensive distance calculations)
- SP-GiST recommendation for point-only KNN workloads (20-40% faster)
- BRIN recommendation for spatially sorted append-only data

**RFC**: 0063 (Spatial Query Optimization)

### 3. pgvector (Vector Similarity Search)

**Architecture**: pgvector adds vector type and two ANN index types:
- HNSW: graph-based, higher recall, more memory
- IVFFlat: inverted file, lower build cost, requires training

DocumentDB also provides vector search via pgvector integration.

**Key Insight**: Vector queries are fundamentally ordering-based (KNN),
not predicate-based. The main optimization decision is pre-filter vs
post-filter when combining vector similarity with scalar predicates.

**Optimization Opportunities**:
- Dimension-aware cost model (cost scales linearly with dimensions)
- Pre-filter vs post-filter strategy selection for hybrid queries
- HNSW vs IVFFlat recommendation based on dataset size and recall needs
- Parameter tuning (m, ef_construction, ef_search, probes, lists)
- Hybrid vector + text search optimization

**RFC**: 0064 (Vector Similarity Search Optimization)

### 4. TimescaleDB (Time-Series)

**Architecture**: TimescaleDB partitions data into "chunks" by time
ranges, provides compression, continuous aggregates, and custom scan
nodes (ChunkAppend).

**Key Insight**: Compressed chunks have fundamentally different I/O
characteristics (5-20x less I/O, 3-5x more CPU). The standard cost
model does not account for this, leading to wrong scan strategy
selection.

**Optimization Opportunities**:
- Chunk-aware cost estimation (compressed vs uncompressed)
- Continuous aggregate matching (rewrite query to use pre-computed view)
- Last-point query optimization (skip-scan for latest value per group)
- Merge-append vs hash-append for multi-chunk ordered queries
- Time-range selectivity estimation using chunk metadata

**RFC**: 0065 (Time-Series Query Optimization)

### 5. Full-Text Search (tsvector, pg_trgm)

**Architecture**: Built-in tsvector/tsquery types with GIN indexes for
boolean matching and GiST indexes for ranking. pg_trgm adds trigram-
based fuzzy matching with its own GIN operator class.

**Key Insight**: Ranking computation (ts_rank) is applied to all
matching rows even for top-N queries. GiST indexes support KNN ordering
that avoids full ranking, but users rarely choose GiST for text search.

**Optimization Opportunities**:
- Ranking deferral (compute rank only for top-N, not all matches)
- GIN vs GiST recommendation (GIN for boolean, GiST for top-N ranking)
- Stored tsvector column recommendation (avoid repeated computation)
- Trigram index advisory for LIKE '%pattern%' queries
- Hybrid text + scalar query optimization via composite GIN (btree_gin)

**RFC**: 0067 (Full-Text Search Optimization)

### 6. Advanced Index Types

**Architecture**: PostgreSQL supports 7 index types (B-tree, Hash, GIN,
GiST, SP-GiST, BRIN, Bloom), each with different cost characteristics
and supported operations.

**Key Insight**: The planner selects between existing indexes but does
not recommend which type to create. DBAs must understand the tradeoffs.
Ra can analyze workload patterns and recommend optimal index types.

**Optimization Opportunities**:
- BRIN index detection (via column correlation statistics)
- GIN operator class recommendation (jsonb_ops vs jsonb_path_ops)
- Covering index suggestion (INCLUDE columns for index-only scans)
- Partial index matching (detect common WHERE clause patterns)
- Multi-index bitmap strategy evaluation
- Bloom index recommendation for multi-column equality filters

**RFC**: 0066 (Advanced Index-Aware Planning)

## Common Patterns Identified

### Pattern 1: Two-Phase Cost Structure

Multiple extensions exhibit a two-phase cost pattern where an index
provides cheap approximate filtering followed by expensive exact
verification:

| Extension | Phase 1 (Cheap) | Phase 2 (Expensive) |
|-----------|----------------|---------------------|
| PostGIS | Bounding box via GiST | Exact geometry test |
| pgvector | ANN index lookup | Exact distance (if needed) |
| pg_trgm | Trigram posting list | Pattern match verification |
| DocumentDB GIN | BSON term lookup | BSON predicate recheck |
| Bloom | Hash probe | Heap fetch + recheck |

Ra should model this two-phase cost structure uniformly across all
index types.

### Pattern 2: Extension-Specific Selectivity

Standard PostgreSQL selectivity estimation fails for extension types:

| Extension | Default Selectivity | Actual Range |
|-----------|-------------------|--------------|
| DocumentDB BSON | 1% (fixed) | 0.001% - 99% |
| PostGIS spatial | Table-level stats only | Geometry-dependent |
| pgvector KNN | Not applicable | Always returns K rows |
| FTS tsvector | Term frequency | 0.01% - 50% |

Ra needs extension-aware selectivity estimators that use type-specific
statistics.

### Pattern 3: Index Type Selection

Each extension has preferred index types, but users often use defaults:

| Extension | Default Choice | Optimal Choice | When |
|-----------|---------------|---------------|------|
| PostGIS | GiST | SP-GiST | Point-only KNN |
| PostGIS | GiST | BRIN | Sorted spatial data |
| FTS | GIN | GiST | Top-N ranking |
| pg_trgm | GIN | GiST | Similarity ordering |
| JSONB | jsonb_ops | jsonb_path_ops | @> only queries |
| pgvector | IVFFlat | HNSW | High-recall needs |

Ra's index advisor should recommend the optimal choice.

### Pattern 4: Custom Scan Interaction

Both DocumentDB and TimescaleDB implement custom scan nodes that
intercede in query execution:

- DocumentDB: `DocumentDBApiQueryScan` for text and vector search
- TimescaleDB: `ChunkAppend` for chunk exclusion

Ra should optimize the plans that feed into these custom scans without
replacing the custom scan logic itself.

## Prioritized Recommendations

### P0 (Immediate Value)

1. **Extension detection API** (RFC 0061): Implement the ExtensionRegistry
   to detect installed extensions. This is the foundation for all
   other optimizations.

2. **DocumentDB selectivity improvement** (RFC 0062): Replace the fixed
   1% selectivity with statistics-based estimation. Highest impact per
   effort due to documentdb's growing adoption.

3. **BRIN index recommendation** (RFC 0066): Simple correlation check
   enables high-value recommendations for time-series and log tables.

### P1 (High Value)

4. **PostGIS two-phase cost model** (RFC 0063): Correct cost estimation
   for spatial queries enables better join ordering.

5. **pgvector pre-filter/post-filter** (RFC 0064): Significant impact
   for AI/ML workloads combining vector search with scalar filters.

6. **FTS ranking optimization** (RFC 0067): Top-N ranking deferral
   provides 10-100x improvement for search workloads.

### P2 (Strategic)

7. **TimescaleDB chunk-aware planning** (RFC 0065): Complements
   TimescaleDB's own optimizer with cross-table optimization.

8. **Covering index suggestion** (RFC 0066): Universal optimization
   that benefits all workloads.

9. **Continuous aggregate matching** (RFC 0065): High value but requires
   deeper TimescaleDB catalog integration.

### P3 (Future)

10. **Cross-extension optimization**: PostGIS + TimescaleDB, pgvector +
    FTS, DocumentDB + pgvector.

11. **Plugin architecture for third-party extensions** (RFC 0061 future
    possibilities).

## RFC References

| RFC | Title | Priority | Status |
|-----|-------|----------|--------|
| 0061 | PostgreSQL Extension-Aware Optimization | Foundation | Proposed |
| 0062 | DocumentDB Query Optimization | P0 | Proposed |
| 0063 | Spatial Query Optimization | P1 | Proposed |
| 0064 | Vector Similarity Search Optimization | P1 | Proposed |
| 0065 | Time-Series Query Optimization | P2 | Proposed |
| 0066 | Advanced Index-Aware Planning | P0-P1 | Proposed |
| 0067 | Full-Text Search Optimization | P1 | Proposed |

## Methodology

Research was conducted by analyzing:
1. Extension source code on GitHub (documentdb, pgvector, PostGIS)
2. PostgreSQL documentation for built-in features (FTS, index types)
3. Extension documentation and wiki pages
4. Existing Ra codebase (`ra-pg-extension`, `ra-core/src/facts.rs`)
5. Existing RFCs (0002, 0021, 0039, 0055, 0056, 0061)

All findings were validated against the actual extension implementations
to ensure optimization rules target real behavior, not assumed behavior.
