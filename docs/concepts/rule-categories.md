# Rule Categories

The RA optimizer organizes its 1,350+ transformation rules into
categories based on the type of optimization they perform.

## Logical Rules

Transform relational algebra expressions while preserving semantics.
These are always safe to apply.

- **Predicate pushdown** -- Move filters closer to data sources
- **Join reordering** -- Change join order for lower cost
- **Projection pruning** -- Remove unnecessary columns early
- **Subquery unnesting** -- Flatten correlated subqueries
- **Constant folding** -- Evaluate constant expressions at compile time
- **Redundant operation elimination** -- Remove no-op operators

## Physical Rules

Map logical operators to physical implementations:

- **Join algorithms** -- Hash join, merge join, nested loop join,
  index nested loop join
- **Access methods** -- Sequential scan, index scan, bitmap scan
- **Sort strategies** -- In-memory sort, external sort
- **Aggregation** -- Hash aggregation, sort aggregation

## Hardware Rules

Optimize for specific hardware capabilities:

- **GPU** -- Offload scans, joins, and aggregations to GPU
- **FPGA** -- Streaming filter pipelines
- **SIMD** -- Vectorized expression evaluation
- **NUMA** -- Data placement and partition affinity

See [Hardware Acceleration](../features/hardware-acceleration.md).

## Distributed Rules

Handle query execution across multiple nodes:

- **Exchange operators** -- Broadcast, shuffle, co-located distribution
- **Partition pruning** -- Skip irrelevant partitions
- **Distributed aggregation** -- Two-phase and three-phase strategies
- **Federated queries** -- Cross-database query planning

See [Distributed Optimization](../features/distributed-optimization.md).

## Multi-Model Rules

Support non-relational data models:

- **Graph traversal** -- Path queries, pattern matching
- **Document queries** -- JSON path, nested document access
- **Time-series** -- Window functions, temporal joins

See [Multi-Model Optimization](../features/multi-model-optimization.md).

## Database-Specific Rules

Optimizations tied to a particular database engine's capabilities
and cost characteristics (PostgreSQL, MySQL, DuckDB, SQLite).
