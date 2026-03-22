# Noria Analysis: Insights for Ra

Date: 2026-03-22
Source: MIT PDOS - https://github.com/mit-pdos/noria
Paper: "Noria: dynamic, partially-stateful data-flow for high-performance web applications" (OSDI 2018)
Author: Jon Gjengset (PhD thesis, MIT)

## Executive Summary

Noria is a streaming dataflow system from MIT that functions as a high-performance storage backend for read-heavy web applications. It precomputes and caches relational query results as materialized views, automatically maintaining them as underlying data changes. Its key innovation is **partially-stateful dataflow**: operators selectively materialize state, evict it under memory pressure, and reconstruct it on demand via "upqueries." Noria achieves 5x throughput over hand-optimized MySQL and scales to tens of millions of reads per second.

This analysis identifies six areas where Noria's techniques directly inform Ra's development: dataflow-based IVM, partial materialization, multi-query computation sharing, delta propagation for joins and aggregations, adaptive state management, and dynamic query migration.

## 1. Architecture Overview

### 1.1 System Model

Noria operates as a server-client system where SQL queries are compiled into a persistent dataflow graph. The system has three layers:

1. **SQL Frontend**: Parses parameterized SQL, produces a Mid-level Intermediate Representation (MIR)
2. **MIR Optimizer**: Rewrites, optimizes, and identifies reuse opportunities across queries
3. **Dataflow Backend**: Executes the graph with domain-partitioned, sharded operators

### 1.2 Key Crate/Module Structure

```
server/
  src/
    controller/       -- Graph management, migration orchestration
      migrate/        -- Live dataflow graph modifications
        materialization/  -- Decides what to materialize
        sharding.rs       -- Auto-sharding decisions
      mir_to_flow.rs  -- MIR-to-dataflow compilation
      recipe/         -- Query set management (add/remove queries)
      sql/            -- SQL parsing and normalization
    builder.rs        -- Dataflow graph construction
    startup.rs        -- Server initialization, ZooKeeper coordination
  dataflow/
    src/
      ops/            -- Dataflow operators
        join.rs       -- Lookup-based incremental join
        filter.rs     -- Predicate evaluation
        project.rs    -- Column selection/computation
        topk.rs       -- Incremental Top-K maintenance
        union.rs      -- Stream merging
        grouped/      -- Aggregation operators
          aggregate.rs      -- SUM, COUNT, AVG
          extremum.rs       -- MIN, MAX
          filteraggregate.rs -- Fused filter+aggregate
      state/          -- Operator state management
        memory_state.rs     -- Multi-indexed in-memory materialization
        persistent_state.rs -- Durable state (RocksDB-backed)
        single_state.rs     -- Single-key state
      domain/         -- Execution partitioning
      backlog/        -- Reader-visible materialized state (evmap)
      processing.rs   -- Delta propagation trait
      group_commit.rs -- Write batching
  mir/
    src/
      optimize.rs     -- MIR-level optimizations
      reuse.rs        -- Cross-query computation sharing
      rewrite.rs      -- Column propagation rewrites
      node.rs         -- MIR node types
```

### 1.3 Dataflow Execution Model

Operators implement the `Ingredient` trait with an `on_input` method that processes a batch of delta records (positive for insertions, negative for deletions) and produces output deltas for downstream consumers. The `ProcessingResult` includes:
- `results`: Output records to propagate
- `misses`: Lookup failures requiring upstream resolution (upqueries)
- `lookups`: State dependencies accessed during processing

Domains are independent execution units containing groups of operators. Domains communicate asynchronously via message passing and can be sharded across threads/machines. A `ChannelCoordinator` manages inter-domain connections.

## 2. Materialized View Maintenance Strategies

### 2.1 Incremental Delta Propagation

Noria uses a **positive/negative record model** for incremental maintenance. Every record carries a polarity flag:
- **Positive (+)**: Row insertion
- **Negative (-)**: Row deletion
- **Update**: Decomposed into negative (old value) + positive (new value)

Deltas propagate through the dataflow graph operator by operator. Each operator transforms input deltas into output deltas according to its semantics:

| Operator | Delta Rule |
|----------|-----------|
| Filter | Forward delta if predicate holds |
| Project | Apply projection to each delta row |
| Join | Lookup matching rows from other side, emit joined deltas |
| Aggregate | Compute incremental aggregate change from delta |
| TopK | Re-evaluate top-k for affected groups |
| Distinct | Track counts; emit positive when 0->1, negative when 1->0 |

**Relevance to Ra**: Ra's RFC 0022 (Incremental View Maintenance) proposes a delta-table approach (separate `_delta_ins` / `_delta_del` tables). Noria's inline positive/negative model is more efficient because deltas flow through operators without intermediate storage. Ra should adopt inline delta polarity rather than separate delta tables.

### 2.2 Incremental Join Maintenance

Noria's join operator uses **indexed lookups** rather than hash join algorithms:

1. Group incoming delta records by join key to batch lookups
2. For each group, perform a single indexed lookup into the other parent's materialized state
3. Combine matching records and emit output deltas
4. For left joins, track unmatched left rows and emit/retract NULLs when right-side state changes

This approach has O(delta * selectivity) cost per update rather than O(base_table_size) for full recomputation. The key insight: **joins need materialized state on at least one side** to process deltas efficiently.

**Relevance to Ra**: Ra's join optimization rules (969 rules across 84 categories) focus on one-shot execution. Incremental join maintenance requires a fundamentally different operator design. Ra should add IVM-aware join rules that:
- Select which side to materialize based on table sizes and update rates
- Optimize join key indexing for incremental lookups
- Propagate delta polarity through join conditions

### 2.3 Incremental Aggregation

Noria implements aggregation incrementally through grouped operators:

- **SUM/COUNT**: Maintain running total, add/subtract delta values
- **MIN/MAX** (extremum): Maintain current extremum. On deletion of the current extremum, query upstream for the new extremum (upquery fallback)
- **AVG**: Maintained via SUM/COUNT pair
- **FilterAggregate**: Fused operator that applies a filter predicate before computing the aggregate, avoiding unnecessary intermediate materialization

The MIN/MAX upquery fallback is a key design decision: when a negative record removes the current minimum, the operator cannot locally determine the new minimum without scanning all values. Noria resolves this by issuing an upquery to the operator's ancestor to replay the full group.

**Relevance to Ra**: Ra's `ra-cache` crate already handles plan cache invalidation via statistics drift. Noria's incremental aggregation approach could inform new Ra optimization rules for maintaining aggregate materialized views. The upquery pattern for MIN/MAX shows that not all aggregates can be maintained purely incrementally -- Ra's IVM implementation should classify aggregates by their maintenance complexity:
- **Algebraic** (SUM, COUNT, AVG): Fully incremental, O(1) per update
- **Holistic** (MIN, MAX, MEDIAN): May require fallback to partial or full recomputation
- **Distributive**: Can be decomposed for distributed incremental maintenance

### 2.4 Top-K Maintenance

Noria's TopK operator maintains the k highest-ranked records per group:

1. Incoming deltas are sorted by group key for batch processing
2. For each affected group, merge new records with existing top-k state
3. Sort the combined set and retain only the top k
4. When deletions shrink a group below k, issue an upquery for replacements

This is directly relevant to queries like `SELECT * FROM t ORDER BY score DESC LIMIT 10` with continuous updates.

**Relevance to Ra**: Ra has `Sort` + `Limit` operators but no fused `TopK` operator. For IVM scenarios, a dedicated TopK operator would be more efficient than sorting the full result set after each update. This could be a new optimization rule: `Sort(Limit(k, input), keys)` -> `TopK(k, keys, input)`.

## 3. Dataflow Optimization Techniques

### 3.1 Filter-Aggregate Fusion

Noria's primary MIR optimization merges consecutive filter and aggregation nodes into a single `FilterAggregation` operator:

1. Depth-first traversal identifies filter-aggregate pairs with a single parent-child edge
2. Validates that the filter predicate does not reference aggregate outputs
3. Reindexes filter column references to the aggregation's input columns
4. Replaces the two-node subgraph with a fused node

This eliminates an intermediate materialization point and reduces the number of operator transitions per record.

**Relevance to Ra**: Ra already has filter pushdown rules. The key insight from Noria is that fusion (not just reordering) eliminates intermediate state. Ra should consider adding operator fusion rules beyond simple pushdown:
- Filter + Aggregate -> FilterAggregate
- Filter + Join -> FilterJoin (predicate applied during join)
- Project + Filter -> FilterProject (avoid materializing unused columns)

### 3.2 Multi-Query Computation Sharing (Reuse)

Noria's `reuse.rs` module identifies opportunities to share computation across queries using a **forward tracing algorithm**:

1. **Base Matching**: Find query base nodes (scans) where the new query's scan is compatible with an existing query's scan
2. **Forward Traversal**: Trace forward from matching bases through child operators. At each step, check if the existing operator can serve the new query's needs (`can_reuse_as()`)
3. **Reuse Node Creation**: Replace reusable subgraphs with `MirNodeType::Reuse` wrappers that reference the existing operator's output
4. **Constraint**: Reuse must follow contiguous paths -- gaps break the chain

This means if queries Q1 and Q2 both scan table `orders` and filter on `status = 'active'`, Noria materializes the filtered result once and both queries read from it.

**Relevance to Ra**: Ra's RFC 0036 (Multi-Query Optimization) proposes hash-based subexpression detection. Noria's forward tracing is simpler and more practical:
- Hash-based detection finds all matches but is expensive (O(n^2) comparisons)
- Forward tracing finds only contiguous-path matches but is O(n) per query
- For an optimizer-as-advisor like Ra (where queries arrive one at a time), Noria's approach may be more practical

Ra should add a **query fingerprinting** system that hashes normalized subplans. When a new query arrives, check if any subplan fingerprints match existing materialized views. This is the advisor version of Noria's reuse detection.

### 3.3 Column Propagation Rewrites

Noria's `rewrite.rs` ensures that operators have access to all columns they need, even when intermediate projections would eliminate them:

- `pull_required_base_columns()`: Traverses bottom-up to identify columns needed by each operator. If a needed column is eliminated by an intermediate projection, adds it back
- This is necessary because Noria's dataflow is long-lived -- columns eliminated at compile time cannot be recovered at runtime

**Relevance to Ra**: Ra's projection pushdown rules should account for this: when pushing projections past joins or filters, ensure all columns needed by downstream operators are preserved. This is standard in query optimizers but becomes more critical in IVM contexts where the dataflow persists.

## 4. Partially-Stateful Dataflow

### 4.1 Core Concept

Traditional dataflow systems require all operators to maintain full state or operate in a purely streaming (windowed) fashion. Noria introduces a middle ground: **partial materialization** where operators selectively maintain state for frequently accessed keys and evict state for cold keys.

This is achieved through three mechanisms:
1. **Selective materialization**: Not all operators need full state
2. **On-demand reconstruction** (upqueries): When a lookup misses, the operator sends an upquery to its ancestors to replay the missing data
3. **Eviction under memory pressure**: Cold entries are evicted, to be reconstructed on the next access

### 4.2 Materialization Decision Algorithm

Noria's materialization planner operates in reverse topological order:

1. **Base tables**: Always fully materialized
2. **Reader nodes** (query outputs): Always materialized (full or partial depending on access patterns)
3. **Intermediate operators**: Materialized only if:
   - They perform self-lookups (e.g., joins need state on at least one side)
   - They are needed as replay sources for downstream partial operators
   - A downstream operator has a lookup obligation that traces back through them

The algorithm ensures no full materializations exist downstream of partial ones (monotonicity constraint).

### 4.3 Upquery Mechanism

When a partial operator receives a lookup for a key that has been evicted:

1. The operator records a "miss" in its `ProcessingResult`
2. The domain scheduler initiates a **replay** from the nearest fully-materialized ancestor
3. The ancestor replays all matching records for the requested key
4. Records flow through intermediate operators, rebuilding state along the path
5. The original lookup is retried

During replay, incoming forward updates are buffered to prevent state corruption. The domain maintains a replay queue with configurable concurrency limits and batch timeouts.

**Relevance to Ra**: Ra's `ra-adaptive` crate implements runtime reoptimization (plan switching at checkpoints). Noria's upquery mechanism is a complementary strategy for a different problem: **state reconstruction**. Ra could adopt partial materialization for its plan cache:
- Hot plans: Fully cached with all statistics
- Warm plans: Cached but with evictable intermediate state
- Cold plans: Only fingerprint cached; full plan reconstructed on demand

### 4.4 Memory Management

Noria's `MemoryState` tracks memory usage precisely via `deep_size_of()` calls. The multi-indexed state structure maintains multiple indices over the same data (each keyed by different columns), with secondary indices reconstructed on demand.

Eviction policies include:
- `AllPartial`: Evict all partial materializations beyond a frontier
- `Readers`: Evict only reader-facing partial state
- `Match(pattern)`: Evict materializations matching name patterns
- Nodes prefixed with `SHALLOW_` are always eligible for eviction

**Relevance to Ra**: Ra's `ra-cache` uses LRU/LFU/adaptive eviction for cached plans. Noria's multi-indexed state approach could inform a more sophisticated caching strategy:
- Multiple access patterns for the same cached plan (by SQL text, by table set, by cost range)
- Selective eviction of plan metadata while retaining the plan structure
- Per-table cache invalidation (Ra already has `clear_table()`, which aligns)

## 5. Caching Strategies

### 5.1 Lock-Free Reader State (evmap)

Noria uses the `evmap` crate for its reader-facing materialized state: a lock-free, eventually-consistent concurrent hashmap with a two-handle pattern:

- **WriteHandle**: Queues changes (positive/negative records)
- **ReadHandle**: Provides lock-free access to the last committed snapshot
- **swap()**: Atomically makes all queued writes visible to readers

This achieves near-zero read latency because readers never contend with writers. The tradeoff is eventual consistency: readers see a snapshot that may lag behind the latest write by one swap interval.

**Relevance to Ra**: Ra's PlanCache uses `Arc<Mutex<>>` for thread safety. For read-heavy workloads (many threads checking the cache, few threads updating plans), a lock-free structure like evmap or a read-write lock would reduce contention. However, Ra's cache entries are larger (full RelExpr plans) and less frequently accessed than Noria's row-level lookups, so the benefit may be marginal.

### 5.2 Group Commit Batching

Noria's `GroupCommitQueueSet` batches writes to amortize per-record overhead:

- Packets are queued per destination domain
- Flushing occurs on timeout expiration or when a batch reaches a size threshold
- Multiple packets to the same destination are merged into a single batch
- `duration_until_flush()` enables efficient scheduling

The batching trade-off is latency vs. throughput: larger batches improve throughput but increase update latency.

**Relevance to Ra**: Ra's plan cache reoptimization (`reoptimize()`) checks all stale plans synchronously. A batched approach inspired by Noria could:
- Accumulate statistics drift events
- Batch reoptimization of related plans (those sharing tables)
- Prioritize reoptimization by plan frequency and drift magnitude

### 5.3 Multi-Index State

Each operator's state can have multiple indices over the same data:

```
MemoryState {
    state: Vec<SingleState>,      // One per index
    by_tag: HashMap<Tag, usize>,  // Replay tag -> index mapping
}
```

When a new query requires looking up a table by a different key, Noria adds a secondary index to the existing state rather than duplicating data. This is done lazily: the new index is constructed by iterating through the primary index on first access.

**Relevance to Ra**: This pattern applies to Ra's catalog and statistics caching. When multiple optimization rules need table statistics indexed differently (by column, by table, by correlation), maintaining multiple indices over the same statistics reduces duplication.

## 6. Architectural Patterns

### 6.1 Domain-Based Execution Partitioning

Noria partitions its dataflow graph into **domains**: independent execution units that can be mapped to threads or machines. Each domain:
- Contains a set of related operators
- Maintains its own state
- Processes messages independently
- Can be sharded into multiple parallel instances

Inter-domain communication uses asynchronous message passing. This enables:
- Thread-per-domain parallelism
- Machine-per-domain distribution
- Independent failure and recovery

**Relevance to Ra**: Ra's `ra-engine` and parallel execution (RFC 0020) use operator-level parallelism. Noria's domain model is coarser-grained, grouping related operators to reduce communication overhead. For Ra's PostgreSQL extension (`ra-pg-extension`), domain-like grouping could reduce the overhead of the Ra optimizer calling back into PostgreSQL for statistics.

### 6.2 Auto-Sharding

Noria automatically shards operators based on data access patterns:

1. **Self-lookup analysis**: Operators that look up their own state must be sharded by the lookup key
2. **Harmonious sharding**: Propagate sharding decisions through compatible operators
3. **Shuffle insertion**: When sharding changes between operators, insert Union (deshard) or Sharder (reshard) nodes

Sharding types: `ByColumn(col, factor)`, `ForcedNone`, `Random`.

**Relevance to Ra**: Ra's distributed optimization rules (58 rules in `distributed/`) handle partition pruning and data movement. Noria's auto-sharding algorithm could inform a new Ra optimization rule that automatically determines optimal partitioning for distributed queries based on join keys and aggregation groups.

### 6.3 Live Migration

Noria supports adding and removing queries at runtime without restarting:

1. **Recipe changes**: New SQL queries are compiled to MIR
2. **Graph augmentation**: New operators are added to the existing dataflow graph
3. **Materialization planning**: Determine what state the new operators need
4. **Routing updates**: Connect new operators to existing data sources
5. **Replay/backfill**: Populate new operator state from existing materializations
6. **Activation**: New query is ready to serve reads

The migration process coordinates across assignment, routing, sharding, transactions, and materialization -- each handled by a dedicated module.

**Relevance to Ra**: Ra's adaptive query execution (RFC 0023) focuses on runtime plan switching. Noria's migration model is broader: it modifies the persistent computation graph. For Ra's PostgreSQL extension, this maps to the ability to add new optimization rules or materialized view advisories without restarting the extension.

## 7. Comparison: Noria vs. Ra

| Dimension | Noria | Ra |
|-----------|-------|-----|
| **Primary role** | Storage backend with auto-maintained views | Query optimizer / advisor |
| **Execution model** | Streaming dataflow (persistent) | One-shot query optimization |
| **View maintenance** | Automatic, incremental via dataflow | Proposed in RFC 0022 (not yet implemented) |
| **Multi-query sharing** | Forward tracing reuse in MIR | Proposed in RFC 0036 (hash-based) |
| **Optimization approach** | MIR-level rewrites (filter-agg fusion) | 969 e-graph rules + cost model |
| **State management** | Partial materialization with upqueries | Plan cache with LRU/LFU eviction |
| **Caching** | Lock-free evmap for readers | Mutex-guarded HashMap |
| **Distribution** | Domain-based sharding + migration | 58 distributed optimization rules |
| **Adaptive behavior** | Partial state reconstruction on demand | Runtime stats + plan switching |
| **Language** | Rust (nightly) | Rust (stable) |

### Key Differences

1. **Noria executes queries; Ra optimizes them.** Noria is a complete storage backend; Ra is an optimizer that advises other databases. This means Ra cannot directly implement Noria's dataflow execution, but can generate optimization rules that achieve similar effects within a traditional RDBMS.

2. **Noria's views are always up-to-date; Ra's cache detects staleness.** Noria maintains views incrementally on every write. Ra's plan cache detects when statistics have drifted and triggers reoptimization. These are complementary: Noria's approach is proactive, Ra's is reactive.

3. **Noria's optimization is simple; Ra's is comprehensive.** Noria has ~3 MIR optimizations (filter-aggregate fusion, filter chain merging, query reuse). Ra has 969 rules across 84 categories. Noria relies on the dataflow model itself for performance; Ra relies on finding the best query plan.

## 8. Optimization Rules Applicable to Ra

Based on the Noria analysis, the following optimization rules and techniques could be added to Ra:

### 8.1 Delta Propagation Rules (for RFC 0022)

```
; Incremental filter maintenance
(rule "IVM: filter delta propagation"
  (IncrementalFilter ?pred (Delta ?input))
  =>
  (Delta (Filter ?pred ?input)))

; Incremental join maintenance (lookup-based)
(rule "IVM: join delta from left"
  (IncrementalJoin ?cond (Delta ?left) (Materialized ?right))
  =>
  (Delta (LookupJoin ?cond ?left ?right)))

; Incremental SUM aggregation
(rule "IVM: SUM delta"
  (IncrementalAggregate SUM ?col (Delta ?input))
  =>
  (DeltaAdd (Sum ?col) ?input))

; MIN/MAX requires upquery fallback on deletion
(rule "IVM: MIN deletion requires full scan"
  (IncrementalAggregate MIN ?col (NegativeDelta ?input))
  =>
  (Upquery (Min ?col) ?input))
```

### 8.2 Operator Fusion Rules

```
; Filter-Aggregate fusion (from Noria optimize.rs)
(rule "fuse filter before aggregate"
  (Aggregate ?group ?agg (Filter ?pred ?input))
  where (not (references-aggregate-output ?pred ?agg))
  =>
  (FilterAggregate ?group ?agg ?pred ?input))

; TopK fusion
(rule "fuse sort-limit into topk"
  (Limit ?k 0 (Sort ?keys ?input))
  =>
  (TopK ?k ?keys ?input))
```

### 8.3 Multi-Query Sharing Rules

```
; Detect shared scan opportunities
(rule "share scan across queries"
  (Query ?q1 (Scan ?table))
  (Query ?q2 (Scan ?table))
  where (!= ?q1 ?q2)
  =>
  (SharedScan ?table [?q1 ?q2]))

; Share filtered scans
(rule "share filtered scan"
  (Query ?q1 (Filter ?pred (Scan ?table)))
  (Query ?q2 (Filter ?pred (Scan ?table)))
  where (!= ?q1 ?q2)
  =>
  (SharedFilter ?pred (SharedScan ?table [?q1 ?q2])))
```

### 8.4 Partial Materialization Advisor Rules

```
; Recommend partial materialization for point lookups
(rule "advise partial materialization"
  (MaterializedView ?name ?def)
  (AccessPattern ?name PointLookup ?key_col)
  (TableSize ?name ?size)
  where (> ?size 1000000)
  =>
  (Advise PartialMaterialization ?name ?key_col
    "Large view with point-lookup access; partial materialization saves memory"))

; Recommend full materialization for scan-heavy views
(rule "advise full materialization"
  (MaterializedView ?name ?def)
  (AccessPattern ?name FullScan)
  =>
  (Advise FullMaterialization ?name
    "Scan-heavy view benefits from full materialization"))
```

## 9. RFC Proposals

### 9.1 Proposed: Dataflow-Based Incremental View Maintenance

**Enhances RFC 0022**

Ra's current RFC 0022 proposes delta tables (`_view_name_delta_ins` / `_view_name_delta_del`). Noria demonstrates that inline delta polarity (positive/negative records flowing through operators) is more efficient. The RFC should be updated to:

1. Define a `DeltaRecord` type with polarity (positive/negative)
2. Define incremental processing semantics for each relational operator
3. Classify aggregates by maintenance complexity (algebraic vs. holistic)
4. Add upquery semantics for holistic aggregates (MIN, MAX, MEDIAN)
5. Specify operator fusion opportunities (FilterAggregate)

### 9.2 Proposed: Partial Materialization Strategy

**New RFC candidate**

Noria's partial materialization balances memory usage against reconstruction cost. Ra could advise PostgreSQL on partial materialization strategies:

- For large materialized views accessed by point lookups, maintain only frequently-accessed key ranges
- Use access pattern tracking to determine hot/cold partitions
- Specify eviction policies (LRU per key range, size-based, time-based)
- Define reconstruction cost models for upquery-like backfill

### 9.3 Proposed: Query Computation Sharing Framework

**Enhances RFC 0036**

Noria's forward tracing reuse algorithm is simpler and more practical than RFC 0036's hash-based approach for common cases. The RFC should incorporate:

1. Forward tracing for contiguous subgraph reuse (fast path)
2. Hash-based detection for non-contiguous sharing (slow path, complex queries)
3. Query fingerprinting for advisor-mode recommendations
4. Cost model for sharing decisions (materialization cost vs. recomputation savings)

## 10. Key Takeaways

### What Ra Should Adopt

1. **Inline delta polarity** for IVM (replace delta tables with positive/negative record model)
2. **Operator fusion rules** (FilterAggregate, TopK) as new optimization rules
3. **Forward tracing reuse detection** as a practical complement to hash-based multi-query optimization
4. **Aggregate classification** (algebraic/holistic/distributive) for IVM planning
5. **Multi-indexed state** pattern for Ra's cache and statistics subsystems

### What Ra Should Not Adopt

1. **Full dataflow execution**: Ra is an optimizer, not a storage engine. Implementing Noria's full dataflow is outside Ra's scope.
2. **Upquery mechanism**: Requires a persistent dataflow graph. Ra's one-shot optimization model doesn't need runtime state reconstruction.
3. **evmap for cache**: Ra's cache access patterns (infrequent writes, moderate reads) don't justify the complexity of lock-free concurrent maps.
4. **ZooKeeper coordination**: Ra's PostgreSQL extension model doesn't need distributed coordination.

### What Ra Already Does Well

1. **Optimization rule breadth**: 969 rules vs. Noria's ~3 MIR optimizations. Ra's e-graph approach is far more powerful for plan optimization.
2. **Cost modeling**: Ra has a sophisticated cost model; Noria has none (it relies on the dataflow model for performance).
3. **Adaptive execution**: Ra's `ra-adaptive` crate (runtime stats, plan switching, checkpoints) is more sophisticated than Noria's fixed dataflow execution.
4. **Hardware awareness**: Ra's `ra-hardware` crate considers CPU, memory, and storage characteristics. Noria does not.

## References

1. Gjengset, J., Schwarzkopf, M., Behrens, J., et al. "Noria: dynamic, partially-stateful data-flow for high-performance web applications." OSDI 2018.
2. Gjengset, J. "Partial State in Dataflow-Based Materialized Views." PhD thesis, MIT, 2020.
3. MIT PDOS. Noria source code. https://github.com/mit-pdos/noria
4. ReadySet (commercial successor). https://github.com/readysettech/readyset
