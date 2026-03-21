# Rule: Reference Table Join Optimization

**Category:** distributed/colocation
**File:** `rules/distributed/colocation/reference-table-join.rra`

## Metadata

- **ID:** `reference-table-join`
- **Version:** "1.0.0"
- **Databases:** citus, cockroachdb, greenplum, trino
- **Tags:** distributed, colocation, reference, replicated, join, local
- **Authors:** "RA Contributors"


# Reference Table Join Optimization

## Description

When one side of a join is a reference table (replicated to every node),
the join executes locally on each node with no data movement. Reference
tables are small, frequently-joined dimension tables that are kept in
sync across all nodes.

**When to apply**: A join involves a reference (replicated) table. The
system maintains a full copy of the reference table on every node.

**Why it works**: Since the reference table is already present on every
node, joining a distributed table with a reference table requires no
exchange operators. Each node joins its local partition of the
distributed table with the local copy of the reference table.

## Relational Algebra

```algebra
Join[c](R_distributed, S_reference)
  -> LocalJoin[c](R_i, S_local)  -- per node i
  -- No exchange needed because S is on every node
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("reference-table-join";
    "(join ?type ?cond ?distributed (exchange broadcast ?ref))" =>
    "(join ?type ?cond ?distributed ?ref)"
    if is_reference_table("?ref")
),

rw!("reference-table-join-no-exchange";
    "(join ?type ?cond
        ?distributed
        (exchange ?any ?ref))" =>
    "(join ?type ?cond ?distributed ?ref)"
    if is_reference_table("?ref")
),
```

## Preconditions

```rust
fn applicable(table: &TableRef) -> bool {
    // Table must be marked as a reference/replicated table
    table.distribution_policy() == DistributionPolicy::Replicated
}
```

**Restrictions:**
- Reference tables consume storage on every node (N copies)
- Updates to reference tables must be synchronously replicated
- Only suitable for small, slowly-changing tables
- In Citus, reference tables have a maximum size limit
- Write amplification: every INSERT/UPDATE/DELETE on a reference table
  must execute on all nodes

## Cost Model

```rust
fn reference_table_join_cost(
    distributed_rows_per_node: f64,
    reference_rows: f64,
) -> f64 {
    // Just the local join cost, no network
    distributed_rows_per_node * (reference_rows).ln()
}

fn vs_broadcast_join_cost(
    distributed_rows_per_node: f64,
    reference_rows: f64,
    reference_bytes: f64,
    num_nodes: u32,
    network_bandwidth: f64,
) -> f64 {
    // Broadcast adds network cost at query time
    let broadcast_cost =
        reference_bytes * num_nodes as f64 / network_bandwidth;
    let join_cost =
        distributed_rows_per_node * (reference_rows).ln();
    broadcast_cost + join_cost
}
```

**Typical benefit**: Eliminates all query-time network cost for
dimension table joins. For a 10-table star schema with 9 dimension
joins, this saves 9 broadcast operations per query.

## Test Cases

```sql
-- Positive: Citus reference table join
-- CREATE REFERENCE TABLE countries;
-- SELECT create_reference_table('countries');
SELECT o.*, c.name
FROM orders o           -- distributed by customer_id
JOIN countries c ON o.country_code = c.code;

-- Plan (no exchange):
-- HashJoin(o.country_code = c.code)
--   Scan(orders)       -- local partition
--   Scan(countries)    -- local reference copy
```

```sql
-- Positive: Greenplum replicated table
-- CREATE TABLE dim_status (...) DISTRIBUTED REPLICATED;
SELECT e.*, s.description
FROM events e
JOIN dim_status s ON e.status_id = s.id;

-- Plan: local join on each segment, no motion operator
```

```sql
-- Negative: large table incorrectly marked as reference
-- A 10GB table replicated to 100 nodes = 1TB total storage
-- Should use hash distribution instead
```

## References

Citus: src/backend/distributed/planner/multi_physical_planner.c - ReferenceTableJoin()
CockroachDB: Multi-region reference tables
Greenplum: src/backend/cdb/cdbllize.c - replicated table handling
Ozcan et al., "Multi-Objective Parametric Query Optimization" (SIGMOD 2009)
