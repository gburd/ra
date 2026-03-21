# Rule: Follower Read Optimization

**Category:** distributed/distributed-transactions
**File:** `rules/distributed/distributed-transactions/follower-read-optimization.rra`

## Metadata

- **ID:** `follower-read-optimization`
- **Version:** "1.0.0"
- **Databases:** cockroachdb, tidb
- **Tags:** distributed, transaction, follower-read, stale, latency, raft
- **Authors:** "RA Contributors"


# Follower Read Optimization

## Description

Routes read queries to the nearest replica (follower) instead of the
Raft leader, when the query can tolerate slightly stale data. In both
CockroachDB and TiDB, data is replicated across nodes using Raft
consensus. Normally, all reads go to the leader to ensure consistency.
Follower reads bypass the leader, reducing latency by reading from a
geographically closer replica.

**When to apply**: The query is a read-only operation that can tolerate
bounded staleness (e.g., AS OF SYSTEM TIME in CockroachDB, or stale
read in TiDB). The follower replica must have applied all Raft log
entries up to the requested timestamp.

**Why it works**: In multi-region deployments, the Raft leader may be
in a distant region. Follower reads eliminate the cross-region round
trip to the leader, reducing read latency from 100-200ms to <10ms
for local reads.

## Relational Algebra

```algebra
-- Standard read: must go to Raft leader
Scan(T) -> LeaderRead(T)

-- Follower read: can go to nearest replica
Scan(T, AS OF SYSTEM TIME '-10s')
  -> FollowerRead(T, nearest_replica, staleness=10s)
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("route-to-follower";
    "(scan ?table ?staleness_bound)" =>
    "(follower_read ?table ?nearest_replica ?staleness_bound)"
    if is_read_only()
    if staleness_bound_specified("?staleness_bound")
    if follower_has_caught_up("?nearest_replica", "?staleness_bound")
),
```

## Preconditions

```rust
fn applicable(
    query: &RelExpr,
    staleness: Option<Duration>,
) -> bool {
    // Must be a read-only query
    query.is_read_only()
    // Must have a staleness bound (explicit or implicit)
    && staleness.is_some()
    // In CockroachDB: AS OF SYSTEM TIME or bounded staleness
    // In TiDB: SET TIDB_READ_STALENESS or START TRANSACTION READ ONLY
    //          AS OF TIMESTAMP
}
```

**Restrictions:**
- Only for read-only queries; writes must go to the leader
- The follower must have applied Raft log entries up to the
  requested timestamp (otherwise it blocks or falls back to leader)
- Bounded staleness means results may not include the very latest
  writes (within the staleness window)
- In CockroachDB, exact staleness (AS OF SYSTEM TIME) vs bounded
  staleness (AS OF SYSTEM TIME with_max_staleness) have different
  routing behavior
- TiDB follower reads require the TiKV follower to have a
  resolved-ts >= the read timestamp

## Cost Model

```rust
fn follower_read_benefit(
    leader_latency_ms: f64,
    follower_latency_ms: f64,
    reads_per_query: f64,
) -> f64 {
    (leader_latency_ms - follower_latency_ms) * reads_per_query
}
```

**Typical benefit**: In a 3-region deployment, leader reads may require
100-200ms cross-region latency. Follower reads from the local region
reduce this to 1-5ms, a 20-200x improvement.

## Test Cases

```sql
-- Positive: CockroachDB exact staleness follower read
SELECT * FROM users
AS OF SYSTEM TIME '-10s'
WHERE id = 42;
-- Reads from nearest replica with 10s staleness

-- Positive: CockroachDB bounded staleness
SELECT * FROM users
AS OF SYSTEM TIME with_max_staleness('10s')
WHERE id = 42;
-- Reads from nearest replica that is within 10s of current time
```

```sql
-- Positive: TiDB stale read
SET TIDB_READ_STALENESS = '-5';
SELECT * FROM orders WHERE customer_id = 100;
-- Reads from TiKV follower with 5s staleness
```

```sql
-- Negative: write operation
INSERT INTO orders VALUES (1, 'new', 100);
-- Must go to Raft leader for consensus
```

```sql
-- Negative: no staleness tolerance
SELECT * FROM inventory WHERE product_id = 5;
-- Without AS OF SYSTEM TIME, reads go to leader for freshness
```

## References

CockroachDB docs: "Follower Reads" and "AS OF SYSTEM TIME"
CockroachDB: pkg/sql/opt/ - bounded staleness optimization
TiDB docs: "Stale Read" and "Follower Read"
TiDB: pkg/session/ - follower read routing
Ongaro & Ousterhout, "In Search of an Understandable Consensus Algorithm" (ATC 2014)
