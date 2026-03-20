# RFC 0006: Distributed Query Optimization

- **Status:** Implemented
- **Type:** Retroactive
- **Author:** RA Contributors
- **Date:** 2026-03-20

---

## Summary

Network-aware cost modeling, data distribution strategies, and
optimization rules for distributed query execution across clusters,
data warehouses, and federated database environments.

## Motivation

Phases 1-18 delivered production-ready single-node optimization.
Modern deployments span multiple nodes (Spark, Presto, Trino),
data warehouses (Snowflake, BigQuery, Redshift), federated setups
across multiple databases, and cloud-native architectures with
network boundaries. The optimizer needed to account for data
movement costs and distribution strategies.

## What Was Built

### Network Cost Modeling

`NetworkTopology` models a graph of nodes connected by links with
bandwidth, latency, and billing cost. `NetworkCostModel` combines
topology with table placement to estimate transfer costs. Five
predefined network profiles cover common deployment patterns.

### Distribution Strategies

The optimizer selects from three strategies for each join:

- **Broadcast**: replicate the smaller table to all nodes
- **Shuffle**: redistribute both tables by join key
- **CoLocated**: use existing data partitioning (no transfer)

Strategy selection accounts for network locality, data skew, and
table sizes.

### Distributed Optimizer

`DistributedOptimizer` rewrites plans with distribution-aware
operators. It inserts `Exchange` nodes for data redistribution
and selects partition-wise execution when applicable.

### Rules

34 rules in `rules/distributed/`:

- `join-distribution/` -- broadcast vs shuffle vs colocated
- `filter-pushdown-distributed/` -- push filters before exchanges
- `aggregation-distribution/` -- two-phase distributed aggregation
- `sort-distribution/` -- distributed sort with merge

### Federated Queries

Support for queries spanning multiple database systems. The
optimizer places operators close to their data and minimizes
cross-system data transfer.

## Key Design Decisions

- Distribution strategy selection is cost-based, not heuristic
- Network topology is explicit configuration rather than
  auto-discovered, supporting capacity planning scenarios
- Exchange operators are first-class plan nodes, not implicit
- Skew detection uses histogram-based analysis to avoid broadcast
  of skewed partitions

## Prior Art

- Spark Catalyst's exchange planning
- Presto/Trino's distributed join strategies
- CockroachDB's distributed SQL optimizer
- Google F1's distributed query execution

## References

- `docs/phase19-distributed-optimization.md` -- full documentation
- `docs/network-modeling.md` -- network cost model details
- `docs/federated-queries.md` -- federated query support
- `crates/ra-core/src/distribution.rs` -- distribution types
- `crates/ra-engine/src/distributed_optimizer.rs` -- optimizer
- `rules/distributed/` -- 34 distribution rules
