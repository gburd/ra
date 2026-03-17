# Distributed Query Optimization Rules

Rules in this directory handle optimization of queries across distributed
database systems. They address data movement, parallelization, and
distribution strategies unique to multi-node execution.

## Directory Structure

- **exchange-placement/** - Exchange (shuffle/broadcast) operator insertion
- **data-movement/** - Data movement minimization strategies
- **distributed-joins/** - Distributed join strategies (broadcast, shuffle, co-located)
- **partial-aggregation/** - Two-phase aggregation (local + global)
- **partition-pruning/** - Partition elimination and pruning
- **distributed-sort/** - Distributed sorting strategies
- **stage-planning/** - Query stage decomposition and planning
- **colocation/** - Data co-location and placement strategies

## Systems Studied

These rules are extracted from and inspired by:

- **Presto/Trino** - Stage-based MPP execution with exchange operators
- **Spark SQL** - Catalyst optimizer with exchange insertion
- **CockroachDB** - Distributed SQL with co-located processing
- **Citus (PostgreSQL)** - Distributed PostgreSQL with reference/distributed tables
- **Greenplum** - MPP analytics with motion operators
- **Apache Calcite** - Framework used by Hive, Flink, Phoenix

## Key Concepts

### Exchange Operators
Exchange operators redistribute data between nodes. The three primary
strategies are:

1. **Gather** - Collect all data to a single node
2. **Repartition (Shuffle)** - Hash-partition data by key(s) across nodes
3. **Broadcast (Replicate)** - Send a full copy to every node

### Distribution Properties
Each operator produces output with specific distribution properties:

- **Singleton** - Data on exactly one node
- **HashPartitioned(keys)** - Hash-distributed on given column(s)
- **RangePartitioned(keys)** - Range-distributed on given column(s)
- **Replicated** - Full copy on every node
- **Random** - Arbitrarily distributed (round-robin, etc.)

### Cost Model Factors
Distributed cost models must account for:

- **Network transfer** - Bytes moved between nodes
- **Serialization** - Encoding/decoding overhead
- **Skew** - Uneven data distribution across nodes
- **Parallelism** - Number of nodes working concurrently
- **Latency** - Network round-trip times

## When to Use

Apply distributed rules when the target system executes queries across
multiple nodes:

```rust
let rules = if config.distributed {
    let mut r = load_rules("distributed/");
    r.extend(load_rules("logical/")); // still apply logical rules
    r
} else {
    load_rules("logical/")
};
```

## System Comparison

| Feature | Presto/Trino | Spark SQL | CockroachDB | Citus | Greenplum |
|---------|-------------|-----------|-------------|-------|-----------|
| Exchange type | Stage boundary | ShuffleExchange | TableReader | Custom scan | Motion |
| Join strategy | Broadcast/Partition | Broadcast/Shuffle | Lookup/Hash | Broadcast/Repartition | Redistribute/Broadcast |
| Partial agg | Yes (stage-local) | Yes (Catalyst) | Yes (local) | Yes (worker-local) | Yes (segment-local) |
| Partition prune | Yes (dynamic) | Yes (dynamic) | Yes (spans) | Yes (shard prune) | Yes (segment prune) |
| Co-location | - | Bucketed tables | Co-located ranges | Reference tables | Replicated tables |

## References

Graefe, "Encapsulation of Parallelism in the Volcano Query Processing System" (SIGMOD 1990)
DeWitt & Gray, "Parallel Database Systems: The Future of High Performance Database Processing" (CACM 1992)
Leis et al., "Morsel-Driven Parallelism" (SIGMOD 2014)
Trino documentation: https://trino.io/docs/current/optimizer.html
Spark SQL documentation: https://spark.apache.org/docs/latest/sql-performance-tuning.html
CockroachDB architecture: https://www.cockroachlabs.com/docs/stable/architecture/overview.html
