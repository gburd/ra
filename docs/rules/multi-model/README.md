# multi-model Rules

Total rules in this category:       30

## Overview

Multi-model rules optimize queries across different data models including document stores, graph databases, and time-series systems.

## Subcategories

- [document](./document/) -       10 rules
- [graph](./graph/) -       10 rules
- [timeseries](./timeseries/) -       10 rules

## Rules

- [Array Unwind Pushdown](./array-unwind-pushdown.md) - `array-unwind-pushdown`
- [Change Stream Filter Pushdown](./change-stream-filter-pushdown.md) - `change-stream-filter-pushdown`
- [Compound Index Selection for Nested Fields](./compound-index-selection.md) - `compound-index-selection`
- [Group Push Accumulator Optimization](./group-push-accumulator.md) - `group-push-accumulator`
- [Lookup to Embedded Document Access](./lookup-to-embedded.md) - `lookup-to-embedded`
- [Nested Predicate Pushdown](./nested-predicate-pushdown.md) - `nested-predicate-pushdown`
- [Aggregation Pipeline Coalescence](./pipeline-coalescence.md) - `pipeline-coalescence`
- [Projection to Covered Query](./projection-to-covered-query.md) - `projection-to-covered-query`
- [Schema Inference Pushdown](./schema-inference-pushdown.md) - `schema-inference-pushdown`
- [Shard Key Targeted Query](./shard-key-targeted-query.md) - `shard-key-targeted-query`
- [Bidirectional Search Optimization](./bidirectional-search.md) - `bidirectional-search`
- [Degree-Aware Join Ordering](./degree-aware-join-ordering.md) - `degree-aware-join-ordering`
- [Expand Into Optimization](./expand-into.md) - `expand-into`
- [Join to Graph Traversal Conversion](./join-to-traversal.md) - `join-to-traversal`
- [Label Scan Pushdown](./label-scan-pushdown.md) - `label-scan-pushdown`
- [Path Materialization for Repeated Traversals](./path-materialization.md) - `path-materialization`
- [Graph Pattern Decomposition](./pattern-decomposition.md) - `pattern-decomposition`
- [Predicate Pushdown Through Traversal](./predicate-pushdown-through-traversal.md) - `predicate-pushdown-through-traversal`
- [Subgraph Isomorphism Pruning](./subgraph-isomorphism-pruning.md) - `subgraph-isomorphism-pruning`
- [Vertex-Centric Index Selection](./vertex-centric-index.md) - `vertex-centric-index`
- [Aligned Aggregation Merge](./aligned-aggregation-merge.md) - `aligned-aggregation-merge`
- [Delta Encoding Aware Scan](./delta-encoding-scan.md) - `delta-encoding-scan`
- [Downsampling Pushdown](./downsampling-pushdown.md) - `downsampling-pushdown`
- [Gap Fill Pushdown](./gap-fill-pushdown.md) - `gap-fill-pushdown`
- [Last Point Query Optimization](./last-point-optimization.md) - `last-point-optimization`
- [Retention Policy Pruning](./retention-policy-pruning.md) - `retention-policy-pruning`
- [Segment Merge Elimination](./segment-merge-elimination.md) - `segment-merge-elimination`
- [Tag Index Scan for Series Filtering](./tag-index-scan.md) - `tag-index-scan`
- [Time Range Pruning](./time-range-pruning.md) - `time-range-pruning`
- [Window Function Pushdown for Time Series](./window-function-pushdown.md) - `window-function-pushdown`
