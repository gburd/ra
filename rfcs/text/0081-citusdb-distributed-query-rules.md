# RFC 0081: CitusDB Distributed Query Optimization Rules

- Start Date: 2026-03-25
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Add Citus-specific distributed query optimization rules to Ra. Citus extends
PostgreSQL with three table types (distributed, reference, local), co-location
groups, shard pruning, and a columnar storage engine. Ra should detect Citus
metadata, model network transfer costs between coordinator and workers, and
apply distribution-aware join, aggregation, and scan strategies that exploit
co-located joins, reference table broadcasts, distributed aggregation pushdown,
and columnar storage characteristics.

## Motivation

Citus is Microsoft's distributed PostgreSQL extension deployed in Azure Cosmos DB
for PostgreSQL and open-source self-hosted clusters. It transforms PostgreSQL
into a horizontally-scaled distributed database by sharding tables across worker
nodes via a distribution column.

Ra's existing `distributed_optimizer.rs` provides generic distribution strategies
(broadcast, shuffle, co-located, partition-wise) but has no awareness of Citus
metadata. This causes three categories of sub-optimal plans:

**1. Missed co-located join opportunities.** When two distributed tables share
the same distribution column and co-location group, their matching shards reside
on the same worker node. A join on the distribution column is purely local with
zero network transfer. Without Citus metadata, Ra may choose shuffle or broadcast
strategies that move data unnecessarily.

**2. Unnecessary reference table transfers.** Reference tables are replicated to
every worker node. Joins between a distributed table and a reference table need
no data movement -- the reference table is already local. Ra currently does not
know a table is a reference table and may estimate broadcast costs.

**3. Suboptimal aggregation strategies.** Citus pushes aggregation to workers
when the GROUP BY includes the distribution column. Without this knowledge, Ra
may plan centralized aggregation that pulls all rows to the coordinator.

**4. Columnar table mis-costing.** Citus columnar tables use compression and
column-oriented storage. Sequential scans are cheap for projection-heavy queries
but expensive for wide reads. The standard row-based cost model overestimates
scan costs for narrow projections on columnar tables.

**Expected gains:**

| Optimization | Scenario | Estimated speedup |
|---|---|---|
| Co-located join detection | Join on distribution key | 10-100x (avoids network) |
| Reference table broadcast skip | Join with dimension table | 2-10x |
| Distributed aggregation pushdown | GROUP BY distribution key | 5-50x |
| Shard pruning | Filter on distribution key | linear in shard count |
| Columnar scan costing | Narrow projection on wide table | 2-5x better estimates |

## Design

### Citus Metadata Model

The optimizer detects Citus by querying `pg_extension` for the `citus` extension.
When present, it reads Citus catalog tables:

- `pg_dist_partition`: distribution column and method per table
- `pg_dist_shard`: shard ranges and placements
- `pg_dist_colocation`: co-location group assignments
- `pg_dist_node`: worker node addresses and ports
- `columnar.options`: columnar storage parameters per table

This metadata is represented as `CitusMetadata`:

```rust
pub struct CitusMetadata {
    pub distributed_tables: HashMap<String, DistributedTableInfo>,
    pub reference_tables: HashSet<String>,
    pub local_tables: HashSet<String>,
    pub colocation_groups: HashMap<u32, Vec<String>>,
    pub shard_count: u32,
    pub worker_nodes: Vec<CitusWorkerNode>,
}

pub struct DistributedTableInfo {
    pub distribution_column: String,
    pub distribution_method: DistributionMethod,
    pub colocation_group: u32,
    pub shard_count: u32,
}
```

### Optimization Rules

**Rule 1: Co-located join detection.** When both sides of a join are distributed
tables in the same co-location group and the join condition includes equality on
both distribution columns, the join is co-located. Cost: zero network transfer.

**Rule 2: Reference table join optimization.** When one side of a join is a
reference table, no data movement is needed -- the reference table is replicated
to every worker. This is strictly cheaper than any broadcast or shuffle.

**Rule 3: Distributed aggregation pushdown.** When GROUP BY includes the
distribution column of the input table, the aggregation can execute on each
worker independently, with only the aggregated results sent to the coordinator.
For decomposable aggregates (SUM, COUNT, MIN, MAX, AVG), partial aggregation
runs on workers and final aggregation on the coordinator.

**Rule 4: Shard pruning via partition key filter.** When a WHERE clause contains
an equality or range predicate on the distribution column, the optimizer
determines which shards are relevant and prunes the rest. This reduces the
number of workers contacted.

**Rule 5: Columnar table cost adjustment.** Columnar tables have different I/O
characteristics: sequential scans read only projected columns, compression
reduces I/O, but random access is expensive. The cost model adjusts based on
the ratio of projected columns to total columns and compression ratio.

### Integration

`CitusOptimizer` wraps the existing `DistributedOptimizer` and adds
Citus-specific rules. It reads Citus metadata, translates it into the existing
`DataDistribution` and `ClusterTopology` types where possible, and adds new
optimization paths for Citus-specific strategies.

## Testing

- Unit tests for each rule (co-located join, reference table, distributed agg,
  shard pruning, columnar costing)
- TPC-H queries on a simulated Citus cluster topology
- Property-based tests for shard pruning correctness
- Benchmark comparing plan quality with and without Citus awareness
