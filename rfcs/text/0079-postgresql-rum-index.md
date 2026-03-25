# RFC 0079: PostgreSQL RUM Index Optimization

- Start Date: 2026-03-25
- Author: Ra Research Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Ra should detect PostgreSQL RUM indexes (an extension of GIN) and exploit
their distance-ordering capability for full-text search with ranking,
timestamp-ordered text queries, and KNN retrieval. RUM stores additional
metadata (positions, timestamps) in posting lists, enabling ordered index
scans that GIN cannot provide. This RFC defines detection, cost modeling,
rewrite rules, and index recommendation logic for RUM.

## Motivation

GIN indexes are the standard PostgreSQL access method for full-text search,
but they have a fundamental limitation: they cannot return results in
relevance order. Queries like "find the 10 most relevant articles" must
fetch all matching rows, compute `ts_rank()` for each, sort, and truncate.
For a query matching 100K rows but returning only 10, this is orders of
magnitude slower than necessary.

The RUM extension (https://github.com/postgrespro/rum) solves this by
storing positional information and additional sortable fields directly in
the posting list entries. This enables:

1. **Distance-ordered scans**: The `<=>` operator returns results sorted
   by relevance distance, so a `LIMIT 10` query touches only ~10 posting
   list entries instead of all matches.

2. **Faster phrase search**: Lexeme positions are stored in the index,
   eliminating heap fetches to verify phrase proximity.

3. **Timestamp-ordered text search**: The `rum_tsvector_addon_ops`
   operator class stores an additional timestamp alongside each posting
   entry, enabling `ORDER BY timestamp` without a separate sort step.

4. **Immediate results**: Depth-first traversal returns initial results
   before scanning the entire posting tree.

**Expected impact:**

| Pattern | GIN | RUM | Gain |
|---------|-----|-----|------|
| Top-10 by ts_rank from 100K matches | Rank all 100K | Distance scan, ~10 fetches | 100-1000x |
| Phrase search (`<->`) | Heap recheck | Index-only verification | 2-5x |
| Text search + ORDER BY timestamp | GIN scan + sort | Single ordered scan | 5-20x |
| KNN text retrieval | Full scan + sort | Distance-ordered scan | 10-100x |

## Guide-level explanation

### RUM index detection

Ra detects RUM availability by checking the `pg_am` catalog for an access
method named `rum`. When present, Ra extends its index type taxonomy to
include RUM and adjusts cost estimates accordingly.

```sql
-- Detection query (executed once per planning session)
SELECT EXISTS(SELECT 1 FROM pg_am WHERE amname = 'rum');
```

### RUM operator classes

Ra recognizes the following RUM operator classes:

| Operator Class | Data Type | Key Feature |
|---------------|-----------|-------------|
| `rum_tsvector_ops` | tsvector | FTS with distance ordering |
| `rum_tsvector_hash_ops` | tsvector | Hash-based FTS (no prefix search) |
| `rum_tsvector_addon_ops` | tsvector + addon | FTS with additional sort field |
| `rum_tsquery_ops` | tsquery | Query-side indexing |
| `rum_anyarray_ops` | anyarray | Array operations with length |
| `rum_TYPE_ops` | int/float/timestamp | Scalar distance ordering |

### Distance ordering operators

RUM introduces three distance operators:

- `<=>`: Bidirectional distance (for relevance ranking)
- `<=|`: Left-side distance (for "items before X")
- `|=>`: Right-side distance (for "items after X")

### Cost model: RUM vs GIN

Ra's cost model accounts for RUM's additional metadata storage:

```
RUM build cost  = GIN build cost * 1.4  (extra metadata per posting entry)
RUM scan cost   = depends on query pattern:
  - Boolean match only: GIN cost * 1.1  (slight overhead from wider postings)
  - Top-N ranked:       GIN cost * 0.01 (distance scan vs full-scan+sort)
  - Phrase search:      GIN cost * 0.4  (no heap recheck for positions)
  - Timestamp-ordered:  GIN cost * 0.05 (no separate sort needed)
```

### Rewrite rules

Ra applies the following rewrites when RUM indexes are detected:

1. **Rank-to-distance**: Rewrite `ORDER BY ts_rank(...) LIMIT N` to
   `ORDER BY tsvector <=> tsquery LIMIT N` when a RUM index exists.

2. **Sort elimination**: Remove explicit `ORDER BY timestamp` when
   `rum_tsvector_addon_ops` provides the ordering natively.

3. **Phrase optimization**: Prefer RUM scan over GIN+heap-recheck for
   phrase search predicates (`<->` operator).

### Example: Top-N ranking optimization

```sql
-- Original query (uses GIN, computes rank for all matches)
SELECT *, ts_rank(body_tsv, q) AS rank
FROM articles, plainto_tsquery('postgresql optimization') AS q
WHERE body_tsv @@ q
ORDER BY rank DESC
LIMIT 10;

-- Rewritten with RUM distance ordering
SELECT *, body_tsv <=> plainto_tsquery('postgresql optimization') AS dist
FROM articles
WHERE body_tsv @@ plainto_tsquery('postgresql optimization')
ORDER BY dist
LIMIT 10;
```

## Reference-level explanation

### Implementation structure

The implementation lives in `crates/ra-engine/src/rum_index.rs` with
integration points in `ra-core`, `ra-stats`, and `ra-pg-extension`.

### RUM index availability detection

```rust
pub struct RumAvailability {
    pub installed: bool,
    pub version: Option<String>,
}

pub fn detect_rum_availability() -> RumAvailability {
    // In pg-extension context: check pg_am for 'rum'
    // In standalone context: check extension metadata
}
```

### Cost model

```rust
pub struct RumCostParams {
    /// Cost per posting list lookup (higher than GIN: wider entries)
    pub term_lookup_cost: f64,       // default: 3.5
    /// Cost per result for distance computation
    pub distance_compute_cost: f64,  // default: 0.3
    /// Cost per result for heap fetch (same as GIN)
    pub heap_fetch_cost: f64,        // default: 1.5
    /// Build cost multiplier relative to GIN
    pub build_cost_multiplier: f64,  // default: 1.4
    /// Cost for phrase position verification (in-index)
    pub phrase_verify_cost: f64,     // default: 0.1
}
```

#### RUM scan cost for ranked queries

```rust
pub fn rum_ranked_scan_cost(
    total_rows: f64,
    selectivity: f64,
    limit: Option<u64>,
    params: &RumCostParams,
) -> f64 {
    let matching = (total_rows * selectivity).max(1.0);
    match limit {
        Some(k) => {
            // Distance-ordered scan: only visit ~k entries
            let k = k as f64;
            let overfetch = k * 1.2; // 20% overfetch for safety
            params.term_lookup_cost
                + overfetch * (params.distance_compute_cost + params.heap_fetch_cost)
        }
        None => {
            // Full scan with distance computation
            params.term_lookup_cost
                + matching * (params.distance_compute_cost + params.heap_fetch_cost)
        }
    }
}
```

#### Comparison with GIN

```rust
pub fn rum_vs_gin_ratio(
    total_rows: f64,
    selectivity: f64,
    query_type: RumQueryType,
    limit: Option<u64>,
    rum_params: &RumCostParams,
    gin_params: &GinBsonCostParams,
) -> f64 {
    let rum_cost = rum_scan_cost(total_rows, selectivity, query_type, limit, rum_params);
    let gin_cost = gin_equivalent_cost(total_rows, selectivity, query_type, limit, gin_params);
    if gin_cost <= 0.0 { return 1.0; }
    rum_cost / gin_cost
}
```

### Query type classification

```rust
pub enum RumQueryType {
    /// Boolean match only (tsvector @@ tsquery)
    BooleanMatch,
    /// Ranked retrieval (ORDER BY ts_rank() or <=> distance)
    RankedRetrieval { limit: Option<u64> },
    /// Phrase search using <-> operator
    PhraseSearch,
    /// Text search with timestamp ordering
    TimestampOrdered,
    /// KNN retrieval using distance operators
    Knn { k: u64 },
}
```

### E-graph rewrite rules

Five rewrite rules are provided:

1. `rum-rank-to-distance`: Convert ts_rank ORDER BY to distance scan
2. `rum-phrase-index-scan`: Prefer RUM for phrase predicates
3. `rum-sort-elimination`: Remove explicit sort when RUM provides order
4. `rum-addon-timestamp-order`: Use addon ops for timestamp ordering
5. `rum-knn-limit-pushdown`: Push LIMIT into RUM KNN scan

### Integration points

#### ra-core::facts::IndexType

Add `Rum` variant to the `IndexType` enum.

#### ra-stats::index_types

Add `IndexCostFactors::rum_default()` with RUM-specific cost parameters.
Add `IndexType::RUM` variant to the ra-stats `IndexType` enum.

#### ra-pg-extension::stats_bridge

Extend `parse_index_type()` to recognize `"rum"` access method name.
Extend `resolve_am_type()` to map RUM OID to the new variant.

### Error handling

All RUM optimizations are non-fatal. When RUM is not installed or a
RUM-specific rewrite fails, the optimizer falls back to GIN-based or
standard planning:

```rust
#[derive(Debug, thiserror::Error)]
pub enum RumError {
    #[error("RUM extension not installed; using GIN cost model")]
    ExtensionNotInstalled,

    #[error("RUM operator class '{opclass}' not recognized; skipping")]
    UnknownOperatorClass { opclass: String },

    #[error("Distance ordering not available for {reason}; falling back")]
    DistanceOrderingUnavailable { reason: String },
}
```

### Performance considerations

- RUM build time is ~40% slower than GIN due to additional metadata
- RUM indexes are ~20-30% larger than GIN due to stored positions
- For boolean-only queries, GIN is slightly faster (narrower postings)
- For ranked/ordered queries, RUM is 10-1000x faster (avoids full scan)

## Drawbacks

**Extension dependency.** RUM is not a core PostgreSQL feature. Users must
install it separately, and it may not be available in managed PostgreSQL
services.

**Build and insert overhead.** RUM indexes are slower to build and update
than GIN. For write-heavy workloads, this trade-off may not be worthwhile.

**Larger index size.** Storing positional data increases index size by
20-30% compared to GIN. For very large tables, this storage overhead
matters.

**Limited operator class support.** Not all GIN operator classes have RUM
equivalents. Ra must correctly identify which queries can use RUM.

## Rationale and alternatives

### Why this design

The design follows the established pattern from `documentdb_optimizer.rs`:
separate cost model, query pattern classification, and e-graph rewrite
rules. This modularity allows RUM support to be enabled/disabled without
affecting other optimization paths.

### Alternative: GiST for KNN text search

GiST supports KNN ordering for tsvector, but is slower for boolean
matching and does not support phrase search. RUM combines GIN's boolean
efficiency with GiST's ordering capability.

### Alternative: Application-level ranking

Applications could fetch all GIN matches and sort client-side. This wastes
network bandwidth and database resources for large result sets.

## Prior art

- **PostgreSQL GIN**: The baseline inverted index. RUM extends GIN's
  posting list format to include additional metadata.
- **Elasticsearch**: Stores term positions and payloads in the inverted
  index, similar to RUM's approach.
- **Apache Lucene**: DocValues provide sorted access to field values,
  analogous to RUM's addon operator class.
- **DocumentDB RUM fork**: Microsoft's DocumentDB uses a forked RUM
  for BSON document indexing (see RFC 0080).

## Unresolved questions

1. Should Ra automatically recommend RUM over GIN when both are available
   and the query pattern benefits from distance ordering?
2. How to handle mixed workloads where some queries benefit from RUM and
   others from GIN on the same column?
3. Should Ra recommend installing RUM when it detects ranking-heavy
   workloads on GIN-indexed columns?

## Future possibilities

- **DocumentDB BSON RUM integration** (RFC 0080): Extend RUM support to
  DocumentDB's BSON operator classes.
- **Hybrid RUM+B-tree indexes**: Combine RUM text search with B-tree
  scalar filtering in a single composite index.
- **RUM index advisor**: Recommend replacing GIN with RUM based on
  observed query patterns (ranking frequency, phrase search usage).
- **Streaming ranked results**: Exploit RUM's depth-first traversal
  for cursor-based pagination of ranked results.
