# Multi-Model Query Optimization

This document describes the multi-model optimization rules and cost
models in the RA system, covering graph, document, and time-series
database patterns.

## Overview

Modern applications often combine multiple data models within a single
system or across polyglot persistence layers. The `ra-multimodel`
crate and `rules/multi-model/` directory provide:

- **30 optimization rules** targeting graph traversal, document query
  pipelines, and time-series scan patterns
- **Model-specific operators** (`GraphOp`, `DocumentOp`,
  `TimeSeriesOp`) that extend the core relational algebra
- **Unified cost model** (`MultiModelCostConfig`) that adjusts cost
  estimates based on physical characteristics of the target database
- **Benefit estimation functions** for deciding when to apply
  cross-model rewrites (e.g., join-to-traversal conversion)

## Architecture

```
                    Query Plan (RelExpr)
                           |
                           v
              ,--------------------------,
              |  Multi-Model Cost Model |
              |   (ra-multimodel crate) |
              `-------+---------+---------+-----'
                   |       |       |
         ,-----------'       |       `------------,
         v                 v                 v
  ,--------------,   ,---------------,   ,--------------,
  |  Graph      |   |  Document   |   | TimeSeries |
  |  Operators  |   |  Operators  |   | Operators  |
  `---------+-------'   `---------+--------'   `--------+--------'
         |                |                 |
         v                v                 v
  ,--------------,   ,---------------,   ,--------------,
  | Neo4j      |   | MongoDB     |   | TimescaleDB|
  | JanusGraph |   | Couchbase   |   | InfluxDB   |
  | Neptune    |   | CosmosDB    |   | QuestDB    |
  `---------------'   `----------------'   `---------------'
```

## Data Models

The `DataModel` enum identifies the target database model:

- **Relational** -- standard SQL (pass-through, no adjustment)
- **Graph** -- property graph databases (reduced IO due to adjacency
  locality, weighted traversal cost)
- **Document** -- document stores (increased IO due to larger reads,
  weighted deserialization cost)
- **TimeSeries** -- columnar time-series stores (reduced IO due to
  compression, weighted decompression cost)

## Graph Optimizations

Graph rules target property graph databases where relationships are
first-class entities stored via adjacency lists rather than join
tables.

### Operators

| Operator | Description |
|----------|-------------|
| `Traverse` | Single-hop edge traversal with direction |
| `VarLengthPath` | Variable-length path with min/max hops |
| `BidirectionalBfs` | BFS from both endpoints toward the middle |
| `ExpandInto` | Check edge existence between two bound nodes |
| `LabelScan` | Scan all nodes with a specific label |
| `VertexCentricScan` | Index scan on edge properties |

### Rules (10)

1. **join-to-traversal** -- Convert equi-joins on foreign keys to
   graph traversals when the join pattern matches a relationship
   lookup. Effective when `avg_degree << right_cardinality`.

2. **bidirectional-search** -- Replace deep unidirectional traversals
   with bidirectional BFS from known endpoints. Reduces explored
   nodes from `O(b^d)` to `O(2 * b^(d/2))`.

3. **path-materialization** -- Materialize frequently-traversed
   variable-length paths into precomputed reachability tables.

4. **vertex-centric-index** -- Use per-vertex edge indexes when
   filtering on edge properties. Converts `O(degree)` scan to
   `O(log(degree) + matches)`.

5. **expand-into** -- When both endpoints of a pattern are already
   bound, use an adjacency check instead of a full traversal.

6. **label-scan-pushdown** -- Push node label predicates into the
   storage engine scan to avoid post-filtering.

7. **pattern-decomposition** -- Break complex graph patterns into
   smaller subpatterns and join results, enabling per-fragment
   optimization.

8. **degree-aware-join-ordering** -- Order pattern matching to start
   from the most selective (lowest degree) node label.

9. **predicate-pushdown-through-traversal** -- Push property
   predicates through traversal operators to filter early.

10. **subgraph-isomorphism-pruning** -- Use structural and label
    constraints to prune the search space during subgraph matching.

### Cost Model

Graph cost estimation accounts for the branching factor (average
degree) and traversal depth:

- **Traversal**: `rows = source_count * (avg_degree * selectivity)^hops`
- **Bidirectional BFS**: `nodes = 2 * branching_factor^(depth/2)`
  (explores far fewer nodes than unidirectional for deep searches)
- **Label scan**: Linear in label count with low constant
- **Vertex-centric index**: `O(log(degree) + selectivity * degree)`
- **Expand-into**: `O(log(degree))` per pair

## Document Optimizations

Document rules target schemaless stores where data is stored as
nested JSON/BSON documents with optional secondary indexes.

### Operators

| Operator | Description |
|----------|-------------|
| `CollectionScan` | Full collection scan |
| `FilteredScan` | Scan with inline predicate on nested fields |
| `IndexOnlyScan` | Covered query returning only indexed fields |
| `Unwind` | Flatten an array field into multiple documents |
| `Lookup` | Cross-collection join ($lookup) |
| `EmbeddedAccess` | Direct access to embedded subdocument |
| `ChangeStreamFiltered` | Change stream with server-side filter |

### Rules (10)

1. **nested-predicate-pushdown** -- Push filter predicates on nested
   fields into the storage engine when a matching index exists on the
   dotted path.

2. **projection-to-covered-query** -- When all projected fields are
   in a compound index, convert to an index-only scan that never
   fetches the full document.

3. **array-unwind-pushdown** -- Push filters applied after `$unwind`
   into the unwind operation to reduce intermediate cardinality.

4. **schema-inference-pushdown** -- Use schema statistics to
   eliminate type-check predicates when the field has a single
   dominant type (>99% of documents).

5. **lookup-to-embedded** -- Replace cross-collection `$lookup` with
   embedded document access when the data model supports denormalized
   embedding. Eliminates the join entirely.

6. **compound-index-selection** -- Select the compound index with
   the best prefix match for multi-field query predicates.

7. **pipeline-coalescence** -- Merge adjacent `$match` and `$project`
   stages into a single stage to reduce pipeline overhead.

8. **shard-key-targeted-query** -- Route queries that include the
   shard key to a single shard instead of broadcasting to all shards.

9. **group-push-accumulator** -- Push accumulator expressions
   (`$sum`, `$avg`) into the `$group` stage to avoid materializing
   intermediate results.

10. **change-stream-filter-pushdown** -- Push predicates into change
    stream subscriptions so the server filters events before sending
    them to the client.

### Cost Model

Document cost estimation accounts for document size and
deserialization overhead:

- **Collection scan**: `CPU = doc_count * 0.1`, `IO = doc_count * avg_size`
- **Filtered scan**: CPU includes evaluation cost, IO is full scan
- **Index-only scan**: IO proportional to `selectivity * index_entry_size`
  (much smaller than full documents)
- **Lookup (indexed)**: `O(outer * log(inner))` vs. unindexed
  `O(outer * inner)`
- **Unwind**: Output rows = `doc_count * avg_array_length`
- **Embedded access**: CPU-only, no extra IO

## Time-Series Optimizations

Time-series rules target databases that partition data into
time-ordered chunks with optional continuous aggregates and
columnar compression.

### Operators

| Operator | Description |
|----------|-------------|
| `ChunkScan` | Scan with time-range pruning |
| `ContinuousAggregateScan` | Read from pre-computed rollups |
| `LastPoint` | Most recent row per series |
| `GapFilledAggregate` | Aggregation with gap filling |
| `TagScan` | Filter by tag (series identifier) column |
| `DeltaScan` | Direct delta-encoded column scan |
| `AlignedAggregate` | Chunk-parallel aligned aggregation |

### Rules (10)

1. **time-range-pruning** -- Eliminate chunks whose time boundaries
   do not overlap the query's time range. Pruning benefit =
   `(total_chunks - matching_chunks) / total_chunks`.

2. **downsampling-pushdown** -- Route aggregation queries to a
   continuous aggregate when the query bucket interval aligns with
   an available pre-computed rollup.

3. **last-point-optimization** -- Convert `GROUP BY series ORDER BY
   time DESC LIMIT 1` into a dedicated last-point scan that uses
   per-series indexes.

4. **gap-fill-pushdown** -- Push gap-fill logic (LOCF, linear
   interpolation, NULL, constant) into the aggregation operator to
   avoid a separate pass.

5. **retention-policy-pruning** -- Skip chunks beyond the retention
   horizon that are scheduled for deletion.

6. **window-function-pushdown** -- Push time-windowed functions
   (moving average, rate) into the chunk scan to compute
   incrementally.

7. **segment-merge-elimination** -- Skip merge of non-overlapping
   segments when the query can be answered from a single segment.

8. **tag-index-scan** -- Use tag indexes to scan only the series
   matching a tag predicate, avoiding a full table scan.

9. **aligned-aggregation-merge** -- Execute aligned aggregations in
   parallel across chunks and merge results. Cost scales as
   `O(rows / parallelism + chunk_count)`.

10. **delta-encoding-scan** -- Read delta-encoded columns directly
    without full decompression when the query only needs deltas or
    rates.

### Cost Model

Time-series cost estimation accounts for chunk structure and
compression:

- **Chunk scan**: `CPU = matching_chunks * rows_per_chunk * 0.05 +
  total_chunks * 0.001` (metadata lookup), `IO = rows * 0.01`
- **Continuous aggregate scan**: `CPU = bucket_count * 0.02`
  (typically orders of magnitude fewer rows than raw scan)
- **Last-point**: `CPU = series_count * 0.1` (one index probe per
  series)
- **Tag scan**: `CPU = matching_series * rows_per_series * 0.05`
- **Aligned aggregate**: `CPU = (rows / parallelism) * 0.08 +
  chunk_count` (merge cost)
- **Gap fill**: `CPU = total_buckets * 0.03`, `IO = data_buckets *
  0.01`

## Unified Cost Adjustment

The `adjust_cost_for_model` function applies model-specific weights
to a base cost estimate:

| Model | CPU Weight | IO Multiplier | Rationale |
|-------|-----------|---------------|-----------|
| Relational | 1.0x | 1.0x | Baseline |
| Graph | `traversal_weight` | 0.5x | Adjacency locality |
| Document | `deserialization_weight` | 1.2x | Larger reads |
| TimeSeries | `compression_weight` | 0.3x | Columnar compression |

Network cost is multiplied by `network_multiplier` for all
non-relational models to account for distributed deployments.

## Benefit Estimation

Three functions help the optimizer decide when to apply cross-model
rewrites:

- **`join_vs_traversal_benefit`**: Compares hash join cost
  `O(left * log(right))` against traversal cost `O(left * degree)`.
  Returns a value in `[0, 1]` where higher means traversal is more
  beneficial.

- **`cagg_vs_raw_benefit`**: Compares raw scan cost against
  continuous aggregate cost. Returns `(raw_rows - bucket_count) /
  raw_rows`. A value > 0.99 means the aggregate is 100x+ cheaper.

- **`covered_query_benefit`**: Compares full document fetch against
  index-only scan. Returns `(doc_size - index_entry_size) /
  doc_size`. A value > 0.9 means the index is 10x+ smaller.

## Crate Structure

```
crates/ra-multimodel/
  src/
    lib.rs          -- module declarations
    graph.rs        -- GraphOp, GraphStats, cost functions (13 tests)
    document.rs     -- DocumentOp, CollectionStats, cost functions (12 tests)
    timeseries.rs   -- TimeSeriesOp, TimeSeriesStats, cost functions (16 tests)
    cost.rs         -- DataModel, MultiModelCostConfig, benefit functions (11 tests)

rules/multi-model/
  graph/            -- 10 .rra rule files
  document/         -- 10 .rra rule files
  timeseries/       -- 10 .rra rule files
```

Total: 52 tests across 4 modules, all passing with zero clippy
warnings.
